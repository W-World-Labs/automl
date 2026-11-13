//! Versioned templates (paper skeletons, reproduce.yaml, skills) + manifest
//! distribution with content-hash ETags.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

pub const KINDS: &[&str] = &["paper", "reproduce", "skill"];

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub body: String,
    pub manifest: Value,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Template {
    /// Strong ETag over the distributable content: manifest + body + version.
    pub fn etag(&self) -> String {
        let payload = format!(
            "{}:{}:{}",
            self.version,
            self.manifest,
            w_crypto::sha256_hex(self.body.as_bytes())
        );
        format!("\"{}\"", w_crypto::sha256_hex(payload.as_bytes()))
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateTemplateRequest {
    pub name: String,
    /// One of `paper` | `reproduce` | `skill`.
    pub kind: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "empty_object")]
    pub manifest: Value,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchTemplateRequest {
    pub name: Option<String>,
    pub body: Option<String>,
    pub manifest: Option<Value>,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

pub async fn create(db: &PgPool, owner: Uuid, req: CreateTemplateRequest) -> ApiResult<Template> {
    if req.name.trim().is_empty() {
        return Err(ApiError::invalid("name must not be empty"));
    }
    if !KINDS.contains(&req.kind.as_str()) {
        return Err(ApiError::invalid(format!(
            "kind must be one of: {}",
            KINDS.join(", ")
        )));
    }
    if !req.manifest.is_object() {
        return Err(ApiError::invalid("manifest must be a JSON object"));
    }
    let row = sqlx::query_as::<_, TemplateRow>(
        r#"
        INSERT INTO templates (name, kind, body, manifest, created_by)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, name, kind, body, manifest, version, created_by, created_at, updated_at
        "#,
    )
    .bind(req.name.trim())
    .bind(&req.kind)
    .bind(&req.body)
    .bind(&req.manifest)
    .bind(owner)
    .fetch_one(db)
    .await?;
    Ok(row.into())
}

pub async fn get(db: &PgPool, id: Uuid) -> ApiResult<Template> {
    let row = sqlx::query_as::<_, TemplateRow>(
        r#"
        SELECT id, name, kind, body, manifest, version, created_by, created_at, updated_at
        FROM templates WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| ApiError::not_found("template not found"))?;
    Ok(row.into())
}

pub async fn list(db: &PgPool, params: &PageParams) -> ApiResult<Page<Template>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, TemplateRow>(
        r#"
        SELECT id, name, kind, body, manifest, version, created_by, created_at, updated_at
        FROM templates
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
        rows.into_iter().map(Template::from).collect(),
        limit,
        |t| super::cursor::encode(t.created_at, t.id),
    ))
}

/// Any content change bumps `version` — manifest consumers cache by ETag.
pub async fn patch(db: &PgPool, id: Uuid, req: PatchTemplateRequest) -> ApiResult<Template> {
    let current = get(db, id).await?;
    if let Some(ref name) = req.name {
        if name.trim().is_empty() {
            return Err(ApiError::invalid("name must not be empty"));
        }
    }
    if let Some(ref manifest) = req.manifest {
        if !manifest.is_object() {
            return Err(ApiError::invalid("manifest must be a JSON object"));
        }
    }
    let name = req.name.as_deref().map(str::trim).unwrap_or(&current.name);
    let body = req.body.as_deref().unwrap_or(&current.body);
    let manifest = req.manifest.as_ref().unwrap_or(&current.manifest);
    let row = sqlx::query_as::<_, TemplateRow>(
        r#"
        UPDATE templates SET
            name = $2, body = $3, manifest = $4,
            version = version + 1, updated_at = now()
        WHERE id = $1
        RETURNING id, name, kind, body, manifest, version, created_by, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(body)
    .bind(manifest)
    .fetch_one(db)
    .await?;
    Ok(row.into())
}

pub async fn delete(db: &PgPool, id: Uuid) -> ApiResult<()> {
    let affected = sqlx::query("DELETE FROM templates WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(ApiError::not_found("template not found"));
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct TemplateRow {
    id: Uuid,
    name: String,
    kind: String,
    body: String,
    manifest: Value,
    version: i32,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TemplateRow> for Template {
    fn from(r: TemplateRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            kind: r.kind,
            body: r.body,
            manifest: r.manifest,
            version: r.version,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_changes_with_content_and_version() {
        let mut t = Template {
            id: Uuid::nil(),
            name: "n".into(),
            kind: "skill".into(),
            body: "b".into(),
            manifest: serde_json::json!({"skills": []}),
            version: 1,
            created_by: Uuid::nil(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let e1 = t.etag();
        assert!(e1.starts_with('"') && e1.ends_with('"'));
        t.version = 2;
        let e2 = t.etag();
        assert_ne!(e1, e2);
        t.body = "changed".into();
        assert_ne!(e2, t.etag());
    }
}
