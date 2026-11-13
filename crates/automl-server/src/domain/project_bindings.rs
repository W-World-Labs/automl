//! Research Hub P2: W-DaaS workspace bindings (`project_binding`).
//!
//! A binding is a one-to-one attach of a project to a downstream DaaS
//! workspace. The row records who bound it and whether the provider verified
//! the workspace; `StubDaaS` can never verify, so `verified` stays false.
//! `research_projects.w_daas_workspace_id` is kept in sync inside the same
//! transaction so the project DTO and the binding log tell one story.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    daas::Daas,
    domain::projects::{self, audit_action, ActorRef},
    error::{ApiError, ApiResult},
};

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectBinding {
    pub id: Uuid,
    pub project_id: Uuid,
    /// Downstream provider; only `w_daas` exists in P2.
    pub provider: String,
    pub workspace_id: String,
    /// `active` | `unbound` (history rows).
    pub status: String,
    /// True when the provider confirmed the workspace exists at bind time.
    pub verified: bool,
    /// Provider-reported display name when verified.
    pub display_name: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub unbound_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BindDaasRequest {
    pub workspace_id: String,
}

/// Response for a bind call: the stored row plus the provider lookup outcome.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct BoundWorkspace {
    #[serde(flatten)]
    pub binding: ProjectBinding,
    /// Provider that answered (`stub` or `daas`).
    pub provider_name: String,
}

const MAX_WORKSPACE_ID_LEN: usize = 256;

fn validate_workspace_id(raw: &str) -> ApiResult<String> {
    let ws = raw.trim();
    if ws.is_empty() || ws.len() > MAX_WORKSPACE_ID_LEN || ws.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "workspace_id must be 1-256 chars without control characters",
        ));
    }
    Ok(ws.to_string())
}

/// Id of the active binding's workspace, or `None` when unbound.
pub async fn active_workspace(pool: &PgPool, project_id: Uuid) -> ApiResult<Option<String>> {
    let ws = sqlx::query_scalar::<_, String>(
        "SELECT workspace_id FROM project_bindings WHERE project_id = $1 AND status = 'active'",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    Ok(ws)
}

pub async fn get_active(pool: &PgPool, project_id: Uuid) -> ApiResult<ProjectBinding> {
    let row = sqlx::query_as::<_, BindingRow>(
        r#"
        SELECT id, project_id, provider, workspace_id, status, verified,
               created_by, created_at, updated_at, unbound_at
        FROM project_bindings
        WHERE project_id = $1 AND status = 'active'
        "#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("no active workspace binding"))?;
    Ok(row.into())
}

/// Bind (or rebind) the project to a DaaS workspace. Rebind retires the prior
/// active row and inserts a new one inside the same transaction that syncs
/// `research_projects.w_daas_workspace_id` and writes `binding.bound` audit.
/// The provider lookup happens before the lock so no row is held over HTTP.
pub async fn bind(
    pool: &PgPool,
    daas: &Daas,
    project_id: Uuid,
    actor: ActorRef,
    req: BindDaasRequest,
) -> ApiResult<BoundWorkspace> {
    let workspace_id = validate_workspace_id(&req.workspace_id)?;
    let resolved = daas.resolve_workspace(&workspace_id).await;

    let mut tx = pool.begin().await?;
    projects::lock_project_for_owner(&mut tx, project_id, actor.scholar_id).await?;
    let replaced = sqlx::query_scalar::<_, String>(
        "UPDATE project_bindings SET status = 'unbound', unbound_at = now(), updated_at = now()
         WHERE project_id = $1 AND status = 'active'
         RETURNING workspace_id",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let row = sqlx::query_as::<_, BindingRow>(
        r#"
        INSERT INTO project_bindings
            (project_id, workspace_id, created_by, verified)
        VALUES ($1, $2, $3, $4)
        RETURNING id, project_id, provider, workspace_id, status, verified,
                  created_by, created_at, updated_at, unbound_at
        "#,
    )
    .bind(project_id)
    .bind(&workspace_id)
    .bind(actor.scholar_id)
    .bind(resolved.verified)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE research_projects SET w_daas_workspace_id = $2, updated_at = now() WHERE id = $1",
    )
    .bind(project_id)
    .bind(&workspace_id)
    .execute(&mut *tx)
    .await?;
    projects::record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::BINDING_BOUND,
        serde_json::json!({
            "workspace_id": workspace_id,
            "verified": resolved.verified,
            "replaced": replaced,
        }),
    )
    .await?;
    tx.commit().await?;

    let mut binding = ProjectBinding::from(row);
    binding.display_name = resolved.display_name;
    Ok(BoundWorkspace {
        binding,
        provider_name: daas.provider_name().to_string(),
    })
}

/// Retire the active binding. 404 when none is bound.
pub async fn unbind(pool: &PgPool, project_id: Uuid, actor: ActorRef) -> ApiResult<()> {
    let mut tx = pool.begin().await?;
    projects::lock_project_for_owner(&mut tx, project_id, actor.scholar_id).await?;
    let removed = sqlx::query_scalar::<_, String>(
        "UPDATE project_bindings SET status = 'unbound', unbound_at = now(), updated_at = now()
         WHERE project_id = $1 AND status = 'active'
         RETURNING workspace_id",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::not_found("no active workspace binding"))?;
    sqlx::query(
        "UPDATE research_projects SET w_daas_workspace_id = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(project_id)
    .execute(&mut *tx)
    .await?;
    projects::record_audit(
        &mut tx,
        project_id,
        actor,
        audit_action::BINDING_UNBOUND,
        serde_json::json!({"workspace_id": removed}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct BindingRow {
    id: Uuid,
    project_id: Uuid,
    provider: String,
    workspace_id: String,
    status: String,
    verified: bool,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    unbound_at: Option<DateTime<Utc>>,
}

impl From<BindingRow> for ProjectBinding {
    fn from(r: BindingRow) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            provider: r.provider,
            workspace_id: r.workspace_id,
            status: r.status,
            verified: r.verified,
            display_name: None,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
            unbound_at: r.unbound_at,
        }
    }
}
