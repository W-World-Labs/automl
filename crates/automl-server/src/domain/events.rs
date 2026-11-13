//! Outbound webhook pipeline: transactional outbox → fan-out → delivery queue.
//!
//! Event sources call [`emit`] INSIDE their business transaction — if the
//! transaction rolls back, the event never existed. A periodic worker runs
//! [`run_once`]: fan out unprocessed outbox rows to matching subscriptions,
//! then attempt every due delivery. Failures back off exponentially
//! (1m/5m/30m/2h/6h) and finally park as `dead` (admin-visible).
//! Nothing on the request path ever performs an outbound call.

use automl_core::{event, Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

/// Retry backoff schedule; after the last step the delivery is dead.
pub const BACKOFF_SECS: [i64; 5] = [60, 300, 1800, 7200, 21600];

/// Delivery machine: `pending → delivered | dead`; both terminal. The worker's
/// SQL (`WHERE status = 'pending'`) is the runtime enforcement; this table is
/// the documented contract (docs/STATE-MACHINES.md) and its unit test.
pub fn delivery_can_transition(from: &str, to: &str) -> bool {
    matches!((from, to), ("pending", "delivered") | ("pending", "dead"))
}

/// Record an event in the outbox, inside the caller's transaction.
pub async fn emit(
    tx: &mut Transaction<'_, Postgres>,
    event_type: &str,
    payload: Value,
) -> Result<(), sqlx::Error> {
    debug_assert!(event::is_known(event_type), "unknown event type emitted");
    sqlx::query("INSERT INTO outbox_events (event_type, payload) VALUES ($1, $2)")
        .bind(event_type)
        .bind(&payload)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// One worker pass: fan out, then deliver. Both halves are independently safe
/// to repeat; concurrent workers skip each other's locked rows.
pub async fn run_once(db: &PgPool, http: &reqwest::Client) -> anyhow::Result<()> {
    fan_out(db).await?;
    deliver_due(db, http).await?;
    Ok(())
}

/// Move unprocessed outbox rows into per-subscription delivery rows.
pub async fn fan_out(db: &PgPool) -> anyhow::Result<usize> {
    let mut tx = db.begin().await?;
    let events: Vec<(Uuid, String, Value, DateTime<Utc>)> = sqlx::query_as(
        r#"
        SELECT id, event_type, payload, occurred_at
        FROM outbox_events
        WHERE processed_at IS NULL
        ORDER BY occurred_at
        LIMIT 50
        FOR UPDATE SKIP LOCKED
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;

    let mut created = 0usize;
    for (event_id, event_type, payload, occurred_at) in &events {
        let subs: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM webhook_subscriptions WHERE active AND $1 = ANY(events)",
        )
        .bind(event_type)
        .fetch_all(&mut *tx)
        .await?;
        for (sub_id,) in subs {
            sqlx::query(
                r#"
                INSERT INTO webhook_deliveries
                    (event_id, subscription_id, event_type, payload, occurred_at)
                VALUES ($1, $2, $3, $4, $5)
                "#,
            )
            .bind(event_id)
            .bind(sub_id)
            .bind(event_type)
            .bind(payload)
            .bind(occurred_at)
            .execute(&mut *tx)
            .await?;
            created += 1;
        }
        sqlx::query("UPDATE outbox_events SET processed_at = now() WHERE id = $1")
            .bind(event_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(created)
}

/// Attempt every due pending delivery once.
pub async fn deliver_due(db: &PgPool, http: &reqwest::Client) -> anyhow::Result<usize> {
    let due: Vec<DueRow> = sqlx::query_as(
        r#"
        SELECT d.id, d.event_id, d.event_type, d.payload, d.attempt, d.occurred_at,
               s.url, s.secret
        FROM webhook_deliveries d
        JOIN webhook_subscriptions s ON s.id = d.subscription_id
        WHERE d.status = 'pending' AND d.next_retry_at <= now()
        ORDER BY d.next_retry_at
        LIMIT 20
        "#,
    )
    .fetch_all(db)
    .await?;

    let mut attempted = 0usize;
    for row in due {
        attempted += 1;
        let body = serde_json::json!({
            "event_id": row.event_id,
            "event_type": row.event_type,
            "occurred_at": row.occurred_at,
            "data": row.payload,
        })
        .to_string();
        let signature = format!(
            "sha256={}",
            w_crypto::hmac_sha256_hex(row.secret.as_bytes(), body.as_bytes())
        );

        let status_code = match http
            .post(&row.url)
            .header("content-type", "application/json")
            .header("x-automl-signature", &signature)
            .header("x-automl-event", &row.event_type)
            .body(body)
            .send()
            .await
        {
            Ok(resp) => Some(resp.status().as_u16() as i32),
            Err(err) => {
                tracing::warn!(delivery = %row.id, error = %err, "webhook delivery transport error");
                None
            }
        };

        let succeeded = status_code.is_some_and(|c| (200..300).contains(&c));
        if succeeded {
            sqlx::query(
                r#"
                UPDATE webhook_deliveries
                SET status = 'delivered', attempt = attempt + 1,
                    last_status_code = $2, updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(row.id)
            .bind(status_code)
            .execute(db)
            .await?;
        } else {
            let next_attempt = row.attempt + 1;
            if (next_attempt as usize) >= BACKOFF_SECS.len() {
                sqlx::query(
                    r#"
                    UPDATE webhook_deliveries
                    SET status = 'dead', attempt = $2, last_status_code = $3,
                        updated_at = now()
                    WHERE id = $1
                    "#,
                )
                .bind(row.id)
                .bind(next_attempt)
                .bind(status_code)
                .execute(db)
                .await?;
            } else {
                let delay = BACKOFF_SECS[next_attempt as usize];
                sqlx::query(
                    r#"
                    UPDATE webhook_deliveries
                    SET attempt = $2, last_status_code = $3,
                        next_retry_at = now() + make_interval(secs => $4),
                        updated_at = now()
                    WHERE id = $1
                    "#,
                )
                .bind(row.id)
                .bind(next_attempt)
                .bind(status_code)
                .bind(delay as f64)
                .execute(db)
                .await?;
            }
        }
    }
    Ok(attempted)
}

#[derive(sqlx::FromRow)]
struct DueRow {
    id: Uuid,
    event_id: Uuid,
    event_type: String,
    payload: Value,
    attempt: i32,
    occurred_at: DateTime<Utc>,
    url: String,
    secret: String,
}

// ── Subscriptions (admin CRUD) ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Subscription {
    pub id: Uuid,
    pub url: String,
    pub events: Vec<String>,
    pub active: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateSubscriptionRequest {
    pub url: String,
    /// HMAC secret the receiver uses to verify `X-Automl-Signature`.
    pub secret: String,
    pub events: Vec<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchSubscriptionRequest {
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub active: Option<bool>,
}

fn validate_url(url: &str) -> ApiResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(ApiError::invalid("url must be http(s)"));
    }
    Ok(())
}

fn validate_events(events: &[String]) -> ApiResult<()> {
    if events.is_empty() {
        return Err(ApiError::invalid("events must not be empty"));
    }
    for e in events {
        if !event::is_known(e) {
            return Err(ApiError::invalid(format!(
                "unknown event type: {e} (known: {})",
                event::ALL.join(", ")
            )));
        }
    }
    Ok(())
}

pub async fn create_subscription(
    db: &PgPool,
    owner: Uuid,
    req: CreateSubscriptionRequest,
) -> ApiResult<Subscription> {
    validate_url(&req.url)?;
    validate_events(&req.events)?;
    if req.secret.len() < 16 {
        return Err(ApiError::invalid("secret must be at least 16 characters"));
    }
    let row = sqlx::query_as::<_, SubRow>(
        r#"
        INSERT INTO webhook_subscriptions (url, secret, events, created_by)
        VALUES ($1, $2, $3, $4)
        RETURNING id, url, events, active, created_by, created_at
        "#,
    )
    .bind(req.url.trim())
    .bind(&req.secret)
    .bind(&req.events)
    .bind(owner)
    .fetch_one(db)
    .await?;
    Ok(row.into())
}

pub async fn list_subscriptions(db: &PgPool, params: &PageParams) -> ApiResult<Page<Subscription>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, SubRow>(
        r#"
        SELECT id, url, events, active, created_by, created_at
        FROM webhook_subscriptions
        WHERE ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
        ORDER BY created_at DESC, id DESC
        LIMIT $3
        "#,
    )
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(db)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(Subscription::from).collect(),
        limit,
        |s| super::cursor::encode(s.created_at, s.id),
    ))
}

pub async fn patch_subscription(
    db: &PgPool,
    id: Uuid,
    req: PatchSubscriptionRequest,
) -> ApiResult<Subscription> {
    if let Some(ref url) = req.url {
        validate_url(url)?;
    }
    if let Some(ref events) = req.events {
        validate_events(events)?;
    }
    let row = sqlx::query_as::<_, SubRow>(
        r#"
        UPDATE webhook_subscriptions SET
            url = COALESCE($2, url),
            events = COALESCE($3, events),
            active = COALESCE($4, active)
        WHERE id = $1
        RETURNING id, url, events, active, created_by, created_at
        "#,
    )
    .bind(id)
    .bind(req.url.as_deref().map(str::trim))
    .bind(req.events.as_deref())
    .bind(req.active)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| ApiError::not_found("subscription not found"))?;
    Ok(row.into())
}

pub async fn delete_subscription(db: &PgPool, id: Uuid) -> ApiResult<()> {
    let affected = sqlx::query("DELETE FROM webhook_subscriptions WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::not_found("subscription not found"));
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct SubRow {
    id: Uuid,
    url: String,
    events: Vec<String>,
    active: bool,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<SubRow> for Subscription {
    fn from(r: SubRow) -> Self {
        Self {
            id: r.id,
            url: r.url,
            events: r.events,
            active: r.active,
            created_by: r.created_by,
            created_at: r.created_at,
        }
    }
}

// ── Deliveries (admin read) ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Delivery {
    pub id: Uuid,
    pub event_id: Uuid,
    pub subscription_id: Uuid,
    pub event_type: String,
    pub status: String,
    pub attempt: i32,
    pub last_status_code: Option<i32>,
    pub next_retry_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_deliveries(
    db: &PgPool,
    params: &PageParams,
    status: Option<&str>,
) -> ApiResult<Page<Delivery>> {
    if let Some(s) = status {
        if !["pending", "delivered", "dead"].contains(&s) {
            return Err(ApiError::invalid("status must be pending|delivered|dead"));
        }
    }
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, DeliveryRow>(
        r#"
        SELECT id, event_id, subscription_id, event_type, status, attempt,
               last_status_code, next_retry_at, created_at
        FROM webhook_deliveries
        WHERE ($1::text IS NULL OR status = $1)
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(status)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(db)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(Delivery::from).collect(),
        limit,
        |d| super::cursor::encode(d.created_at, d.id),
    ))
}

#[derive(sqlx::FromRow)]
struct DeliveryRow {
    id: Uuid,
    event_id: Uuid,
    subscription_id: Uuid,
    event_type: String,
    status: String,
    attempt: i32,
    last_status_code: Option<i32>,
    next_retry_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl From<DeliveryRow> for Delivery {
    fn from(r: DeliveryRow) -> Self {
        Self {
            id: r.id,
            event_id: r.event_id,
            subscription_id: r.subscription_id,
            event_type: r.event_type,
            status: r.status,
            attempt: r.attempt,
            last_status_code: r.last_status_code,
            next_retry_at: r.next_retry_at,
            created_at: r.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule_matches_design() {
        assert_eq!(BACKOFF_SECS, [60, 300, 1800, 7200, 21600]);
    }

    #[test]
    fn delivery_states_are_terminal() {
        assert!(delivery_can_transition("pending", "delivered"));
        assert!(delivery_can_transition("pending", "dead"));
        assert!(!delivery_can_transition("delivered", "pending"));
        assert!(!delivery_can_transition("dead", "pending"));
        assert!(!delivery_can_transition("delivered", "dead"));
    }

    #[test]
    fn url_and_event_validation() {
        assert!(validate_url("https://x.test/hook").is_ok());
        assert!(validate_url("ftp://x").is_err());
        assert!(validate_events(&["paper.decided".into()]).is_ok());
        assert!(validate_events(&[]).is_err());
        assert!(validate_events(&["paper.exploded".into()]).is_err());
    }
}
