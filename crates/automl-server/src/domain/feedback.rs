//! Feedback: anonymous-friendly inbox with admin triage.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

pub const CATEGORIES: &[&str] = &["bug", "feature", "question", "other"];
pub const STATUSES: &[&str] = &["new", "triaged", "closed"];

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Feedback {
    pub id: Uuid,
    pub category: String,
    pub body: String,
    pub contact: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateFeedbackRequest {
    /// One of `bug` | `feature` | `question` | `other`.
    pub category: String,
    pub body: String,
    #[serde(default)]
    pub contact: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchFeedbackRequest {
    /// One of `new` | `triaged` | `closed`.
    pub status: String,
}

pub async fn create(db: &PgPool, req: CreateFeedbackRequest) -> ApiResult<Feedback> {
    if !CATEGORIES.contains(&req.category.as_str()) {
        return Err(ApiError::invalid(format!(
            "category must be one of: {}",
            CATEGORIES.join(", ")
        )));
    }
    if req.body.trim().is_empty() {
        return Err(ApiError::invalid("body must not be empty"));
    }
    if req.body.len() > 10_000 {
        return Err(ApiError::invalid("body too long (max 10000 bytes)"));
    }
    let row = sqlx::query_as::<_, FeedbackRow>(
        r#"
        INSERT INTO feedback (category, body, contact)
        VALUES ($1, $2, $3)
        RETURNING id, category, body, contact, status, created_at
        "#,
    )
    .bind(&req.category)
    .bind(req.body.trim())
    .bind(req.contact.as_deref())
    .fetch_one(db)
    .await?;
    Ok(row.into())
}

pub async fn list(
    db: &PgPool,
    params: &PageParams,
    status: Option<&str>,
) -> ApiResult<Page<Feedback>> {
    if let Some(s) = status {
        if !STATUSES.contains(&s) {
            return Err(ApiError::invalid("status must be new|triaged|closed"));
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
    let rows = sqlx::query_as::<_, FeedbackRow>(
        r#"
        SELECT id, category, body, contact, status, created_at
        FROM feedback
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
        rows.into_iter().map(Feedback::from).collect(),
        limit,
        |f| super::cursor::encode(f.created_at, f.id),
    ))
}

/// Triage flow: `new → triaged → closed`, plus fast-close (`new → closed`)
/// and reopen (`closed → triaged`). Going back to `new` is not a thing.
pub fn can_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("new", "triaged") | ("new", "closed") | ("triaged", "closed") | ("closed", "triaged")
    )
}

pub async fn set_status(db: &PgPool, id: Uuid, req: PatchFeedbackRequest) -> ApiResult<Feedback> {
    if !STATUSES.contains(&req.status.as_str()) {
        return Err(ApiError::invalid("status must be new|triaged|closed"));
    }
    let current: Option<(String,)> = sqlx::query_as("SELECT status FROM feedback WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?;
    let (from,) = current.ok_or_else(|| ApiError::not_found("feedback not found"))?;
    if from != req.status && !can_transition(&from, &req.status) {
        return Err(ApiError::conflict(format!(
            "illegal feedback transition: {from} → {}",
            req.status
        )));
    }
    let row = sqlx::query_as::<_, FeedbackRow>(
        r#"
        UPDATE feedback SET status = $2 WHERE id = $1
        RETURNING id, category, body, contact, status, created_at
        "#,
    )
    .bind(id)
    .bind(&req.status)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| ApiError::not_found("feedback not found"))?;
    Ok(row.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triage_transitions() {
        assert!(can_transition("new", "triaged"));
        assert!(can_transition("new", "closed"));
        assert!(can_transition("triaged", "closed"));
        assert!(can_transition("closed", "triaged"));
        assert!(!can_transition("triaged", "new"));
        assert!(!can_transition("closed", "new"));
    }
}

#[derive(sqlx::FromRow)]
struct FeedbackRow {
    id: Uuid,
    category: String,
    body: String,
    contact: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
}

impl From<FeedbackRow> for Feedback {
    fn from(r: FeedbackRow) -> Self {
        Self {
            id: r.id,
            category: r.category,
            body: r.body,
            contact: r.contact,
            status: r.status,
            created_at: r.created_at,
        }
    }
}
