//! Research Hub P2: project asset pointers (`project_asset`) and the
//! project-scoped hybrid search entry.
//!
//! Pointer kinds (`kg_slice`, `research_draft`, `experiment_spec`,
//! `sim_config`, `result_bundle`, `figure`, `notebook_ptr`) must carry a
//! `uri` or `daas_artifact_id`; content kinds (`note`, `search_snapshot`)
//! may carry their payload in `metadata` alone — the DB CHECK mirrors this.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    daas::{Daas, DaasHit},
    domain::projects::{self, audit_action, ActorRef},
    error::{ApiError, ApiResult},
};

const MAX_POINTER_LEN: usize = 1024;
const MAX_TITLE_LEN: usize = 512;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_QUERY_LEN: usize = 512;
const MAX_SEARCH_LIMIT: u32 = 50;

/// Asset kinds from design §12.3. Serialized snake_case on the wire and as
/// the `kind` TEXT column value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Note,
    SearchSnapshot,
    KgSlice,
    ResearchDraft,
    ExperimentSpec,
    SimConfig,
    ResultBundle,
    Figure,
    NotebookPtr,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::SearchSnapshot => "search_snapshot",
            Self::KgSlice => "kg_slice",
            Self::ResearchDraft => "research_draft",
            Self::ExperimentSpec => "experiment_spec",
            Self::SimConfig => "sim_config",
            Self::ResultBundle => "result_bundle",
            Self::Figure => "figure",
            Self::NotebookPtr => "notebook_ptr",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        Ok(match s {
            "note" => Self::Note,
            "search_snapshot" => Self::SearchSnapshot,
            "kg_slice" => Self::KgSlice,
            "research_draft" => Self::ResearchDraft,
            "experiment_spec" => Self::ExperimentSpec,
            "sim_config" => Self::SimConfig,
            "result_bundle" => Self::ResultBundle,
            "figure" => Self::Figure,
            "notebook_ptr" => Self::NotebookPtr,
            other => return Err(ApiError::invalid(format!("unknown asset kind: {other}"))),
        })
    }

    /// Pointer kinds must carry `uri` or `daas_artifact_id`; content kinds
    /// may be metadata-only.
    fn requires_pointer(self) -> bool {
        !matches!(self, Self::Note | Self::SearchSnapshot)
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectAsset {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: AssetKind,
    pub title: String,
    pub uri: Option<String>,
    pub daas_artifact_id: Option<String>,
    pub metadata: Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddProjectAssetRequest {
    pub kind: AssetKind,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub daas_artifact_id: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Persist the outcome as a `search_snapshot` asset (Member+ only).
    #[serde(default)]
    pub persist: bool,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectSearchResponse {
    pub project_id: Uuid,
    pub workspace_id: String,
    /// Provider that answered: `stub` or `daas`.
    pub provider: String,
    pub query: String,
    pub hits: Vec<DaasHit>,
    /// Set when `persist: true` — the created `search_snapshot` asset.
    pub snapshot_asset_id: Option<Uuid>,
}

fn clean_pointer(raw: Option<&str>, field: &str) -> ApiResult<Option<String>> {
    let Some(value) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if value.len() > MAX_POINTER_LEN || value.chars().any(char::is_control) {
        return Err(ApiError::invalid(format!(
            "{field} must be ≤{MAX_POINTER_LEN} chars without control characters"
        )));
    }
    Ok(Some(value.to_string()))
}

fn clean_title(raw: Option<&str>) -> ApiResult<String> {
    let title = raw.unwrap_or_default().trim();
    if title.len() > MAX_TITLE_LEN || title.chars().any(char::is_control) {
        return Err(ApiError::invalid(format!(
            "title must be ≤{MAX_TITLE_LEN} chars without control characters"
        )));
    }
    Ok(title.to_string())
}

fn clean_metadata(raw: Option<Value>) -> ApiResult<Value> {
    let metadata = raw.unwrap_or_else(|| Value::Object(Default::default()));
    if !metadata.is_object() {
        return Err(ApiError::invalid("metadata must be a JSON object"));
    }
    let size = serde_json::to_vec(&metadata)
        .map_err(|_| ApiError::invalid("metadata must be serializable JSON"))?
        .len();
    if size > MAX_METADATA_BYTES {
        return Err(ApiError::invalid("metadata exceeds 64 KiB"));
    }
    Ok(metadata)
}

/// Lock the project row (serializing against deletion/membership churn),
/// recheck liveness, and confirm the actor is Member or Owner — shared by
/// every asset write path.
async fn lock_for_member_write(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    actor_id: Uuid,
) -> ApiResult<()> {
    let live = sqlx::query_scalar::<_, bool>(
        "SELECT status <> 'deleted' FROM research_projects WHERE id=$1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(false);
    if !live {
        return Err(ApiError::not_found("project not found"));
    }
    let member = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_members
         WHERE project_id = $1 AND principal_type = 'scholar' AND scholar_id = $2
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(actor_id)
    .fetch_optional(&mut **tx)
    .await?;
    if !matches!(member.as_deref(), Some("owner" | "member")) {
        return Err(ApiError::forbidden(
            "requires project role member or higher",
        ));
    }
    Ok(())
}

pub async fn add(
    pool: &PgPool,
    project_id: Uuid,
    actor: ActorRef,
    req: AddProjectAssetRequest,
) -> ApiResult<ProjectAsset> {
    let title = clean_title(req.title.as_deref())?;
    let uri = clean_pointer(req.uri.as_deref(), "uri")?;
    let daas_artifact_id = clean_pointer(req.daas_artifact_id.as_deref(), "daas_artifact_id")?;
    let metadata = clean_metadata(req.metadata)?;
    if req.kind.requires_pointer() && uri.is_none() && daas_artifact_id.is_none() {
        return Err(ApiError::invalid(format!(
            "kind '{}' requires a uri or daas_artifact_id pointer",
            req.kind.as_str()
        )));
    }

    let mut tx = pool.begin().await?;
    lock_for_member_write(&mut tx, project_id, actor.scholar_id).await?;
    let row = sqlx::query_as::<_, AssetRow>(
        r#"
        INSERT INTO project_assets
            (project_id, kind, title, uri, daas_artifact_id, metadata, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, project_id, kind, title, uri, daas_artifact_id, metadata,
                  created_by, created_at, updated_at
        "#,
    )
    .bind(project_id)
    .bind(req.kind.as_str())
    .bind(&title)
    .bind(&uri)
    .bind(&daas_artifact_id)
    .bind(&metadata)
    .bind(actor.scholar_id)
    .fetch_one(&mut *tx)
    .await?;
    projects::record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::ASSET_ADDED,
        serde_json::json!({"asset_id": row.id, "kind": req.kind.as_str()}),
    )
    .await?;
    tx.commit().await?;
    Ok(row.into())
}

pub async fn list(
    pool: &PgPool,
    project_id: Uuid,
    kind: Option<AssetKind>,
    params: &PageParams,
) -> ApiResult<Page<ProjectAsset>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (ts, id) = match params.cursor.as_deref() {
        Some(c) => {
            let (t, i) = super::cursor::decode(c)?;
            (Some(t), Some(i))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, AssetRow>(
        r#"
        SELECT id, project_id, kind, title, uri, daas_artifact_id, metadata,
               created_by, created_at, updated_at
        FROM project_assets
        WHERE project_id = $1 AND deleted_at IS NULL
          AND ($5::text IS NULL OR kind = $5)
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(project_id)
    .bind(ts)
    .bind(id)
    .bind(fetch)
    .bind(kind.map(AssetKind::as_str))
    .fetch_all(pool)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(ProjectAsset::from).collect(),
        limit,
        |a| super::cursor::encode(a.created_at, a.id),
    ))
}

pub async fn get(pool: &PgPool, project_id: Uuid, asset_id: Uuid) -> ApiResult<ProjectAsset> {
    let row = sqlx::query_as::<_, AssetRow>(
        r#"
        SELECT id, project_id, kind, title, uri, daas_artifact_id, metadata,
               created_by, created_at, updated_at
        FROM project_assets
        WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(project_id)
    .bind(asset_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("asset not found"))?;
    Ok(row.into())
}

/// Soft-delete a live asset row; 404 when already gone.
pub async fn remove(
    pool: &PgPool,
    project_id: Uuid,
    asset_id: Uuid,
    actor: ActorRef,
) -> ApiResult<()> {
    let mut tx = pool.begin().await?;
    lock_for_member_write(&mut tx, project_id, actor.scholar_id).await?;
    let removed = sqlx::query(
        "UPDATE project_assets SET deleted_at = now(), updated_at = now()
         WHERE project_id = $1 AND id = $2 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(asset_id)
    .execute(&mut *tx)
    .await?;
    if removed.rows_affected() == 0 {
        return Err(ApiError::not_found("asset not found"));
    }
    projects::record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::ASSET_REMOVED,
        serde_json::json!({"asset_id": asset_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Project-scoped hybrid search. Requires an active workspace binding; the
/// provider answers or fails 503 (a stub answers an empty page honestly).
/// `persist` writes a `search_snapshot` asset under the Member+ recheck.
pub async fn search(
    pool: &PgPool,
    daas: &Daas,
    project_id: Uuid,
    actor: ActorRef,
    req: ProjectSearchRequest,
) -> ApiResult<ProjectSearchResponse> {
    let query = req.query.trim();
    if query.is_empty() || query.len() > MAX_QUERY_LEN || query.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "query must be 1-512 chars without control characters",
        ));
    }
    let limit = req.limit.unwrap_or(10).clamp(1, MAX_SEARCH_LIMIT);
    let workspace_id = super::project_bindings::active_workspace(pool, project_id)
        .await?
        .ok_or_else(|| {
            ApiError::conflict("project has no active DaaS workspace binding; bind one first")
        })?;
    let hits = daas.hybrid_search(&workspace_id, query, limit).await?;

    let snapshot_asset_id = if req.persist {
        let mut tx = pool.begin().await?;
        lock_for_member_write(&mut tx, project_id, actor.scholar_id).await?;
        let metadata = serde_json::json!({
            "query": query,
            "provider": daas.provider_name(),
            "workspace_id": workspace_id,
            "hits": hits,
        });
        let row = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO project_assets
                (project_id, kind, title, metadata, created_by)
            VALUES ($1, 'search_snapshot', $2, $3, $4)
            RETURNING id
            "#,
        )
        .bind(project_id)
        .bind(format!("search: {query}"))
        .bind(&metadata)
        .bind(actor.scholar_id)
        .fetch_one(&mut *tx)
        .await?;
        projects::record_audit(
            &mut tx,
            project_id,
            actor,
            audit_action::ASSET_ADDED,
            serde_json::json!({"asset_id": row, "kind": "search_snapshot"}),
        )
        .await?;
        tx.commit().await?;
        Some(row)
    } else {
        None
    };

    Ok(ProjectSearchResponse {
        project_id,
        workspace_id,
        provider: daas.provider_name().to_string(),
        query: query.to_string(),
        hits,
        snapshot_asset_id,
    })
}

#[derive(FromRow)]
struct AssetRow {
    id: Uuid,
    project_id: Uuid,
    kind: String,
    title: String,
    uri: Option<String>,
    daas_artifact_id: Option<String>,
    metadata: Value,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AssetRow> for ProjectAsset {
    fn from(r: AssetRow) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            kind: AssetKind::parse(&r.kind).unwrap_or(AssetKind::Note),
            title: r.title,
            uri: r.uri,
            daas_artifact_id: r.daas_artifact_id,
            metadata: r.metadata,
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
    fn kind_wire_values_round_trip() {
        for kind in [
            "note",
            "search_snapshot",
            "kg_slice",
            "research_draft",
            "experiment_spec",
            "sim_config",
            "result_bundle",
            "figure",
            "notebook_ptr",
        ] {
            let parsed = AssetKind::parse(kind).unwrap();
            assert_eq!(parsed.as_str(), kind);
        }
        assert!(AssetKind::parse("bogus").is_err());
        assert!(!AssetKind::Note.requires_pointer());
        assert!(!AssetKind::SearchSnapshot.requires_pointer());
        assert!(AssetKind::KgSlice.requires_pointer());
    }
}
