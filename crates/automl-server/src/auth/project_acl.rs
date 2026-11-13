//! Project-level ACL (docs/RESEARCH-HUB-DESIGN.md §12.1).
//!
//! Effective permission = global scope ∩ project role. Handlers check the
//! global wire scope first (`auth.require_scope`), then call here for the
//! project half — both gates must pass. Centralizing the role half means a
//! API-key requests act as the key owner's scholar, limited by the key's
//! scopes. P0 has no authenticated agent principal or credential-to-agent
//! binding; an agent membership row is not request authorization.

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::projects::{self, ProjectRole},
    error::{ApiError, ApiResult},
};

/// Require the caller's project membership at `min_role` or higher.
///
/// Errors: `404` when the project is missing or soft-deleted; `403` when the
/// caller is not a member or their role is below `min_role`.
pub async fn require_project_role(
    db: &PgPool,
    auth: &AuthContext,
    project_id: Uuid,
    min_role: ProjectRole,
) -> ApiResult<ProjectRole> {
    projects::ensure_live(db, project_id).await?;
    match projects::member_role(db, project_id, auth.owner_id()).await? {
        Some(role) if role >= min_role => Ok(role),
        Some(_) => Err(ApiError::forbidden(format!(
            "requires project role {} or higher",
            min_role.as_str()
        ))),
        None => Err(ApiError::forbidden("not a project member")),
    }
}

/// Governance helper for routes that need an immediate ACL decision.
/// Mutation handlers should check the wire scope here and let the domain
/// transaction recheck live state and Owner membership under project lock.
/// API keys represent their owner scholar; they are not agent credentials.
pub async fn require_project_govern(
    db: &PgPool,
    auth: &AuthContext,
    project_id: Uuid,
) -> ApiResult<()> {
    auth.require_scope(automl_core::scope::PROJECTS_ADMIN)?;
    require_project_role(db, auth, project_id, ProjectRole::Owner).await?;
    Ok(())
}
