//! API key lifecycle: mint (`amk_…`), store sha256, verify with ct_eq.

use automl_core::{scope, Page, PageParams};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

/// Wire prefix for Auto.ml API keys (v2). Distinct from v1's `aml_`.
pub const KEY_PREFIX: &str = "amk_";

/// Metadata returned by list / create (never includes hash or plaintext).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ApiKeyMeta {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Create response: meta + plaintext key (returned **once**).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Full plaintext key. Shown only in the create response.
    pub key: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateKeyRequest {
    pub name: String,
    /// Subset of the caller's scopes (admin may grant any known scope).
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct VerifiedKey {
    pub key_id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
}

/// `amk_<first 8 hex of key uuid>_<32-byte urlsafe secret>`.
pub fn mint_plaintext(key_id: Uuid) -> String {
    let simple = key_id.simple().to_string();
    let id_prefix = &simple[..8];
    let mut secret = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    let secret_b64 = URL_SAFE_NO_PAD.encode(secret);
    format!("{KEY_PREFIX}{id_prefix}_{secret_b64}")
}

pub fn hash_key(plaintext: &str) -> [u8; 32] {
    w_crypto::sha256(plaintext.as_bytes())
}

/// Parse the 8-char key-id prefix from a bearer token, if the shape matches.
pub fn key_id_prefix(token: &str) -> Option<&str> {
    let rest = token.strip_prefix(KEY_PREFIX)?;
    let (prefix, secret) = rest.split_once('_')?;
    if prefix.len() == 8 && prefix.chars().all(|c| c.is_ascii_hexdigit()) && !secret.is_empty() {
        Some(prefix)
    } else {
        None
    }
}

pub async fn create_key(
    pool: &PgPool,
    owner_id: Uuid,
    req: CreateKeyRequest,
    caller_scopes: &[String],
    caller_is_admin: bool,
) -> ApiResult<ApiKeyCreated> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::invalid("name must not be empty"));
    }
    if req.scopes.is_empty() {
        return Err(ApiError::invalid("scopes must not be empty"));
    }
    for s in &req.scopes {
        if !scope::is_known(s) {
            return Err(ApiError::invalid(format!("unknown scope: {s}")));
        }
        if !caller_is_admin && !caller_scopes.iter().any(|c| c == s) {
            return Err(ApiError::forbidden(format!(
                "cannot grant scope you do not hold: {s}"
            )));
        }
    }
    if let Some(exp) = req.expires_at {
        if exp <= Utc::now() {
            return Err(ApiError::invalid("expires_at must be in the future"));
        }
    }

    let key_id = Uuid::new_v4();
    let plaintext = mint_plaintext(key_id);
    let digest = hash_key(&plaintext);

    let row = sqlx::query_as::<_, KeyRow>(
        r#"
        INSERT INTO api_keys (id, owner_id, name, key_sha256, scopes, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, name, scopes, expires_at, last_used_at, created_at
        "#,
    )
    .bind(key_id)
    .bind(owner_id)
    .bind(name)
    .bind(digest.as_slice())
    .bind(&req.scopes)
    .bind(req.expires_at)
    .fetch_one(pool)
    .await?;

    Ok(ApiKeyCreated {
        id: row.id,
        name: row.name,
        scopes: row.scopes,
        expires_at: row.expires_at,
        last_used_at: row.last_used_at,
        created_at: row.created_at,
        key: plaintext,
    })
}

pub async fn list_keys(
    pool: &PgPool,
    owner_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<ApiKeyMeta>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;

    let rows = if let Some(cursor) = params.cursor.as_deref() {
        let (created_at, id) = decode_cursor(cursor)?;
        sqlx::query_as::<_, KeyRow>(
            r#"
            SELECT id, name, scopes, expires_at, last_used_at, created_at
            FROM api_keys
            WHERE owner_id = $1
              AND revoked_at IS NULL
              AND (created_at, id) < ($2, $3)
            ORDER BY created_at DESC, id DESC
            LIMIT $4
            "#,
        )
        .bind(owner_id)
        .bind(created_at)
        .bind(id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, KeyRow>(
            r#"
            SELECT id, name, scopes, expires_at, last_used_at, created_at
            FROM api_keys
            WHERE owner_id = $1
              AND revoked_at IS NULL
            ORDER BY created_at DESC, id DESC
            LIMIT $2
            "#,
        )
        .bind(owner_id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    };

    let page = Page::from_rows(
        rows.into_iter().map(ApiKeyMeta::from).collect(),
        limit,
        |m| encode_cursor(m.created_at, m.id),
    );
    Ok(page)
}

/// Soft-revoke. Idempotent for the owner: already-revoked → 204; unknown id → 404.
pub async fn revoke_key(pool: &PgPool, owner_id: Uuid, key_id: Uuid) -> ApiResult<()> {
    let res = sqlx::query(
        r#"
        UPDATE api_keys
        SET revoked_at = now()
        WHERE id = $1 AND owner_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(key_id)
    .bind(owner_id)
    .execute(pool)
    .await?;

    if res.rows_affected() > 0 {
        return Ok(());
    }

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM api_keys WHERE id = $1 AND owner_id = $2)")
            .bind(key_id)
            .bind(owner_id)
            .fetch_one(pool)
            .await?;

    if exists {
        Ok(()) // already revoked
    } else {
        Err(ApiError::not_found("api key not found"))
    }
}

/// Verify a bearer token. Expired or revoked keys return `None` (caller maps to 401).
pub async fn verify_key(pool: &PgPool, token: &str) -> ApiResult<Option<VerifiedKey>> {
    if !token.starts_with(KEY_PREFIX) {
        return Ok(None);
    }
    let digest = hash_key(token);

    let row = sqlx::query_as::<_, VerifyRow>(
        r#"
        SELECT id, owner_id, name, scopes, key_sha256, expires_at, revoked_at
        FROM api_keys
        WHERE key_sha256 = $1
        "#,
    )
    .bind(digest.as_slice())
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    // Constant-time re-check even after the unique-index hit.
    if !w_crypto::ct_eq(row.key_sha256.as_slice(), digest.as_slice()) {
        return Ok(None);
    }
    if row.revoked_at.is_some() {
        return Ok(None);
    }
    if row.expires_at.is_some_and(|exp| exp <= Utc::now()) {
        return Ok(None);
    }

    // last_used_at is best-effort and must never block the request path.
    let pool = pool.clone();
    let key_id = row.id;
    tokio::spawn(async move {
        let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
            .bind(key_id)
            .execute(&pool)
            .await;
    });

    Ok(Some(VerifiedKey {
        key_id: row.id,
        owner_id: row.owner_id,
        name: row.name,
        scopes: row.scopes,
    }))
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    id: Uuid,
    name: String,
    scopes: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<KeyRow> for ApiKeyMeta {
    fn from(r: KeyRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            scopes: r.scopes,
            expires_at: r.expires_at,
            last_used_at: r.last_used_at,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct VerifyRow {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    scopes: Vec<String>,
    key_sha256: Vec<u8>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

fn encode_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    format!("{}|{}", created_at.to_rfc3339(), id)
}

fn decode_cursor(cursor: &str) -> ApiResult<(DateTime<Utc>, Uuid)> {
    let (ts, id) = cursor
        .split_once('|')
        .ok_or_else(|| ApiError::invalid("invalid cursor"))?;
    let created_at = DateTime::parse_from_rfc3339(ts)
        .map_err(|_| ApiError::invalid("invalid cursor timestamp"))?
        .with_timezone(&Utc);
    let id = Uuid::parse_str(id).map_err(|_| ApiError::invalid("invalid cursor id"))?;
    Ok((created_at, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_shape_is_amk_prefix_secret() {
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        let key = mint_plaintext(id);
        assert!(key.starts_with("amk_12345678_"));
        assert_eq!(key_id_prefix(&key), Some("12345678"));
        // Secret may contain `_` (urlsafe alphabet) — take everything after the
        // fixed `amk_<8 hex>_` head, not rsplit.
        let secret = key.strip_prefix("amk_12345678_").unwrap();
        // 32 bytes urlsafe-no-pad → 43 chars
        assert_eq!(secret.len(), 43);
        assert_eq!(URL_SAFE_NO_PAD.decode(secret).unwrap().len(), 32);
    }

    #[test]
    fn hash_is_32_bytes() {
        assert_eq!(hash_key("amk_test").len(), 32);
    }

    #[test]
    fn key_id_prefix_rejects_garbage() {
        assert!(key_id_prefix("aml_old").is_none());
        assert!(key_id_prefix("amk_short_x").is_none());
        assert!(key_id_prefix("amk_12345678_").is_none());
        assert!(key_id_prefix("amk_12345678_secret").is_some());
    }
}
