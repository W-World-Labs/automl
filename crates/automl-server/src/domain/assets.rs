//! Paper assets via presigned upload — server never proxies bytes.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use w_storage::Storage;

use crate::domain::reproduce::validate_hex64;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    Pending,
    Committed,
}

impl AssetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Committed => "committed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "committed" => Self::Committed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Asset {
    pub id: Uuid,
    pub paper_id: Uuid,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub content_type: String,
    pub status: AssetStatus,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAssetRequest {
    pub filename: String,
    pub sha256: String,
    pub size_bytes: i64,
    #[serde(default = "default_ct")]
    pub content_type: String,
}

fn default_ct() -> String {
    "application/octet-stream".into()
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AssetCreated {
    pub asset: Asset,
    pub upload_url: String,
}

pub async fn create_pending(
    pool: &PgPool,
    storage: &Arc<dyn Storage>,
    paper_id: Uuid,
    created_by: Uuid,
    req: CreateAssetRequest,
) -> ApiResult<AssetCreated> {
    if req.filename.trim().is_empty() {
        return Err(ApiError::invalid("filename must not be empty"));
    }
    if req.size_bytes < 0 {
        return Err(ApiError::invalid("size_bytes must be >= 0"));
    }
    validate_hex64("sha256", &req.sha256)?;
    let sha = req.sha256.to_ascii_lowercase();

    let paper_ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM papers WHERE id = $1)")
        .bind(paper_id)
        .fetch_one(pool)
        .await?;
    if !paper_ok {
        return Err(ApiError::not_found("paper not found"));
    }

    let id = Uuid::new_v4();
    let safe_name = sanitize_filename(&req.filename);
    let object_key = format!("papers/{paper_id}/assets/{id}/{safe_name}");

    let row = sqlx::query_as::<_, AssetRow>(
        r#"
        INSERT INTO assets (id, paper_id, object_key, sha256, size_bytes, content_type, status, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7)
        RETURNING id, paper_id, object_key, sha256, size_bytes, content_type, status, created_by, created_at
        "#,
    )
    .bind(id)
    .bind(paper_id)
    .bind(&object_key)
    .bind(&sha)
    .bind(req.size_bytes)
    .bind(&req.content_type)
    .bind(created_by)
    .fetch_one(pool)
    .await?;

    let upload_url = storage
        .presign_put(
            &object_key,
            Some(&req.content_type),
            Duration::from_secs(900),
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "presign_put failed");
            ApiError::unavailable("storage presign failed")
        })?;

    Ok(AssetCreated {
        asset: row.into(),
        upload_url,
    })
}

pub async fn commit(pool: &PgPool, storage: &Arc<dyn Storage>, asset_id: Uuid) -> ApiResult<Asset> {
    let row = sqlx::query_as::<_, AssetRow>(
        r#"
        SELECT id, paper_id, object_key, sha256, size_bytes, content_type, status, created_by, created_at
        FROM assets WHERE id = $1
        "#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("asset not found"))?;

    if row.status == "committed" {
        return Ok(row.into());
    }

    let exists = storage.exists(&row.object_key).await.map_err(|e| {
        tracing::error!(error = %e, "storage exists failed");
        ApiError::unavailable("storage check failed")
    })?;
    if !exists {
        return Err(ApiError::conflict(
            "object not found in storage; upload first",
        ));
    }

    let (bytes, _) = storage.get(&row.object_key).await.map_err(|e| {
        tracing::error!(error = %e, "storage get failed");
        ApiError::unavailable("storage read failed")
    })?;

    let actual = w_crypto::sha256_hex(&bytes);
    if actual != row.sha256.trim() {
        return Err(ApiError::conflict(format!(
            "sha256 mismatch: expected {}, got {actual}",
            row.sha256.trim()
        )));
    }
    if bytes.len() as i64 != row.size_bytes {
        return Err(ApiError::conflict(format!(
            "size mismatch: expected {}, got {}",
            row.size_bytes,
            bytes.len()
        )));
    }

    let updated = sqlx::query_as::<_, AssetRow>(
        r#"
        UPDATE assets SET status = 'committed'
        WHERE id = $1
        RETURNING id, paper_id, object_key, sha256, size_bytes, content_type, status, created_by, created_at
        "#,
    )
    .bind(asset_id)
    .fetch_one(pool)
    .await?;
    Ok(updated.into())
}

pub async fn get(pool: &PgPool, asset_id: Uuid) -> ApiResult<Asset> {
    let row = sqlx::query_as::<_, AssetRow>(
        r#"
        SELECT id, paper_id, object_key, sha256, size_bytes, content_type, status, created_by, created_at
        FROM assets WHERE id = $1
        "#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("asset not found"))?;
    Ok(row.into())
}

fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "blob".into()
    } else {
        cleaned
    }
}

#[derive(sqlx::FromRow)]
struct AssetRow {
    id: Uuid,
    paper_id: Uuid,
    object_key: String,
    sha256: String,
    size_bytes: i64,
    content_type: String,
    status: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<AssetRow> for Asset {
    fn from(r: AssetRow) -> Self {
        Self {
            id: r.id,
            paper_id: r.paper_id,
            object_key: r.object_key,
            sha256: r.sha256.trim().to_string(),
            size_bytes: r.size_bytes,
            content_type: r.content_type,
            status: AssetStatus::parse(&r.status),
            created_by: r.created_by,
            created_at: r.created_at,
        }
    }
}
