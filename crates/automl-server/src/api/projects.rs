//! HTTP handlers for Research Hub projects and their P1 fuel shelf.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    auth::{project_acl, AuthContext, Principal},
    domain::project_assets::{
        self, AddProjectAssetRequest, AssetKind, ProjectAsset, ProjectSearchRequest,
        ProjectSearchResponse,
    },
    domain::project_bindings::{self, BindDaasRequest, BoundWorkspace, ProjectBinding},
    domain::project_papers::{self, AddProjectPaperRequest, ImportPdfRequest, ProjectPaper},
    domain::projects::{
        self, ActorRef, AddMemberRequest, AuditEvent, CreateProjectRequest, PatchProjectRequest,
        ProjectMember, ProjectRole, ProjectVisibility, ResearchProject,
    },
    error::ApiResult,
    state::AppState,
};

/// Member responses retain the full project DTO; public nonmember responses
/// use the deliberately narrow metadata allowlist below.
#[derive(Debug, Serialize, ToSchema)]
#[serde(untagged)]
pub enum ProjectView {
    Member(ResearchProject),
    PublicMeta(PublicProjectMeta),
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicProjectMeta {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub visibility: ProjectVisibility,
    pub status: crate::domain::projects::ProjectStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ResearchProject> for PublicProjectMeta {
    fn from(project: ResearchProject) -> Self {
        Self {
            id: project.id,
            slug: project.slug,
            title: project.title,
            summary: project.summary,
            visibility: project.visibility,
            status: project.status,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

fn actor(auth: &AuthContext) -> ActorRef {
    match &auth.principal {
        Principal::Scholar { scholar_id, .. } => ActorRef::scholar(*scholar_id),
        Principal::ApiKey { owner_id, .. } => ActorRef::api_key(*owner_id),
    }
}

#[utoipa::path(
    post, path = "/research-projects", tag = "projects",
    request_body = CreateProjectRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 201, description = "Project created", body = ResearchProject),
        (status = 409, description = "Slug already used by this owner"),
    )
)]
pub async fn create_project(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateProjectRequest>,
) -> ApiResult<(StatusCode, Json<ResearchProject>)> {
    auth.require_scope(scope::PROJECTS_WRITE)?;
    let project = projects::create(&state.db, actor(&auth), req).await?;
    Ok((StatusCode::CREATED, Json(project)))
}

#[utoipa::path(
    get, path = "/research-projects", tag = "projects",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of projects the caller belongs to"))
)]
pub async fn list_projects(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<ResearchProject>>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    Ok(Json(
        projects::list_mine(&state.db, auth.owner_id(), &params).await?,
    ))
}

#[utoipa::path(
    get, path = "/research-projects/{id}", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, body = ProjectView),
        (status = 403, description = "Private/lab project requires membership"),
    )
)]
pub async fn get_project(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ProjectView>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    let project = projects::get(&state.db, id).await?;
    // Public nonmembers receive only the explicit metadata projection. A
    // member receives the full DTO; private/lab remain members-only.
    if project.visibility == ProjectVisibility::PublicMeta {
        if projects::member_role(&state.db, id, auth.owner_id())
            .await?
            .is_some()
        {
            return Ok(Json(ProjectView::Member(project)));
        }
        return Ok(Json(ProjectView::PublicMeta(PublicProjectMeta::from(
            project,
        ))));
    }
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    Ok(Json(ProjectView::Member(project)))
}

#[utoipa::path(
    patch, path = "/research-projects/{id}", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = PatchProjectRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, body = ResearchProject),
        (status = 409, description = "Illegal status transition"),
    )
)]
pub async fn patch_project(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchProjectRequest>,
) -> ApiResult<Json<ResearchProject>> {
    auth.require_scope(scope::PROJECTS_WRITE)?;
    // The domain transaction checks live state and Owner membership against
    // the locked project row. This layer checks only the global wire scope.
    if req.status.is_some() || req.visibility.is_some() {
        auth.require_scope(scope::PROJECTS_ADMIN)?;
    }
    Ok(Json(
        projects::patch(&state.db, id, actor(&auth), req).await?,
    ))
}

#[utoipa::path(
    post, path = "/research-projects/{id}/members", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = AddMemberRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 201, description = "Member added", body = ProjectMember),
        (status = 409, description = "Already a member"),
        (status = 501, description = "Agent ACL enrollment is deferred in P0"),
    )
)]
pub async fn add_member(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<AddMemberRequest>,
) -> ApiResult<(StatusCode, Json<ProjectMember>)> {
    auth.require_scope(scope::PROJECTS_ADMIN)?;
    let member = projects::add_member(&state.db, id, actor(&auth), req).await?;
    Ok((StatusCode::CREATED, Json(member)))
}

#[utoipa::path(
    get, path = "/research-projects/{id}/members", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of project members"))
)]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<ProjectMember>>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    Ok(Json(projects::list_members(&state.db, id, &params).await?))
}

#[utoipa::path(
    delete, path = "/research-projects/{id}/members/{member_id}", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("member_id" = Uuid, Path, description = "Member row id"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 204, description = "Member removed"),
        (status = 409, description = "Cannot remove the last owner"),
    )
)]
pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthContext,
    Path((id, member_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::PROJECTS_ADMIN)?;
    projects::remove_member(&state.db, id, member_id, actor(&auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/research-projects/{id}/audit", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Append-only audit trail (newest first)"))
)]
pub async fn list_audit(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<AuditEvent>>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    Ok(Json(projects::list_audit(&state.db, id, &params).await?))
}

#[utoipa::path(
    post, path = "/research-projects/{id}/papers", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = AddProjectPaperRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, description = "Paper added to project shelf", body = ProjectPaper),
              (status = 400, description = "Exactly one primary identifier is required"),
              (status = 403, description = "Requires projects:write and project Member or Owner"),
              (status = 409, description = "DOI already exists on project shelf"))
)]
pub async fn add_project_paper(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    body: Result<Json<AddProjectPaperRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ProjectPaper>)> {
    auth.require_scope(scope::PROJECTS_WRITE)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Member).await?;
    let Json(req) =
        body.map_err(|_| crate::error::ApiError::invalid("invalid paper request body"))?;
    let paper =
        project_papers::add(&state.db, &state.paper_enrichment, id, actor(&auth), req).await?;
    Ok((StatusCode::CREATED, Json(paper)))
}

#[utoipa::path(
    get, path = "/research-projects/{id}/papers", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, description = "Page of project shelf papers", body = PageProjectPaper),
        (status = 403, description = "Requires projects:read and project membership"),
    )
)]
pub async fn list_project_papers(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<ProjectPaper>>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    Ok(Json(project_papers::list(&state.db, id, &params).await?))
}

/// OpenAPI shape of the shared pagination envelope.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
struct PageProjectPaper {
    data: Vec<ProjectPaper>,
    next_cursor: Option<String>,
}

// ── P2: DaaS workspace binding ──────────────────────────────────────────────

#[utoipa::path(
    put, path = "/research-projects/{id}/daas/binding", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = BindDaasRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, description = "Workspace bound (rebind replaces the active row)", body = BoundWorkspace),
        (status = 400, description = "Invalid workspace_id"),
        (status = 403, description = "Requires projects:admin and project Owner"),
    )
)]
pub async fn bind_daas(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    body: Result<Json<BindDaasRequest>, JsonRejection>,
) -> ApiResult<Json<BoundWorkspace>> {
    auth.require_scope(scope::PROJECTS_ADMIN)?;
    let Json(req) = body.map_err(|_| crate::error::ApiError::invalid("invalid binding body"))?;
    Ok(Json(
        project_bindings::bind(&state.db, &state.daas, id, actor(&auth), req).await?,
    ))
}

#[utoipa::path(
    get, path = "/research-projects/{id}/daas/binding", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, body = ProjectBinding),
        (status = 403, description = "Requires projects:read and project membership"),
        (status = 404, description = "No active workspace binding"),
    )
)]
pub async fn get_daas_binding(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ProjectBinding>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    Ok(Json(project_bindings::get_active(&state.db, id).await?))
}

#[utoipa::path(
    delete, path = "/research-projects/{id}/daas/binding", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 204, description = "Binding retired"),
        (status = 403, description = "Requires projects:admin and project Owner"),
        (status = 404, description = "No active workspace binding"),
    )
)]
pub async fn unbind_daas(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::PROJECTS_ADMIN)?;
    project_bindings::unbind(&state.db, id, actor(&auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── P2: project assets ──────────────────────────────────────────────────────

#[utoipa::path(
    post, path = "/research-projects/{id}/assets", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = AddProjectAssetRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 201, description = "Asset registered", body = ProjectAsset),
        (status = 400, description = "Invalid kind or missing required pointer"),
        (status = 403, description = "Requires projects:write and project Member or Owner"),
    )
)]
pub async fn add_project_asset(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    body: Result<Json<AddProjectAssetRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ProjectAsset>)> {
    auth.require_scope(scope::PROJECTS_WRITE)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Member).await?;
    let Json(req) = body.map_err(|_| crate::error::ApiError::invalid("invalid asset body"))?;
    let asset = project_assets::add(&state.db, id, actor(&auth), req).await?;
    Ok((StatusCode::CREATED, Json(asset)))
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct AssetListQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    /// Filter by asset kind (e.g. `note`, `search_snapshot`).
    pub kind: Option<String>,
}

#[utoipa::path(
    get, path = "/research-projects/{id}/assets", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
        ("kind" = Option<String>, Query, description = "Filter by asset kind"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, description = "Page of project assets", body = PageProjectAsset),
        (status = 400, description = "Unknown asset kind or malformed cursor"),
        (status = 403, description = "Requires projects:read and project membership"),
    )
)]
pub async fn list_project_assets(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Query(q): Query<AssetListQuery>,
) -> ApiResult<Json<Page<ProjectAsset>>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    let kind = q.kind.as_deref().map(AssetKind::parse).transpose()?;
    let params = PageParams {
        limit: q.limit,
        cursor: q.cursor,
    };
    Ok(Json(
        project_assets::list(&state.db, id, kind, &params).await?,
    ))
}

#[utoipa::path(
    get, path = "/research-projects/{id}/assets/{asset_id}", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("asset_id" = Uuid, Path, description = "Asset id"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, body = ProjectAsset),
        (status = 403, description = "Requires projects:read and project membership"),
        (status = 404, description = "Asset not found"),
    )
)]
pub async fn get_project_asset(
    State(state): State<AppState>,
    auth: AuthContext,
    Path((id, asset_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ProjectAsset>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    Ok(Json(project_assets::get(&state.db, id, asset_id).await?))
}

#[utoipa::path(
    delete, path = "/research-projects/{id}/assets/{asset_id}", tag = "projects",
    params(
        ("id" = Uuid, Path, description = "Project id"),
        ("asset_id" = Uuid, Path, description = "Asset id"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 204, description = "Asset soft-deleted"),
        (status = 403, description = "Requires projects:write and project Member or Owner"),
        (status = 404, description = "Asset not found"),
    )
)]
pub async fn delete_project_asset(
    State(state): State<AppState>,
    auth: AuthContext,
    Path((id, asset_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::PROJECTS_WRITE)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Member).await?;
    project_assets::remove(&state.db, id, asset_id, actor(&auth)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── P2: PDF intake + hybrid search entry ────────────────────────────────────

#[utoipa::path(
    post, path = "/research-projects/{id}/papers/from-pdf", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ImportPdfRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 201, description = "PDF registered as a shelf entry via the bound DaaS workspace", body = ProjectPaper),
        (status = 400, description = "Invalid request"),
        (status = 403, description = "Requires projects:write and project Member or Owner"),
        (status = 409, description = "No active DaaS workspace binding"),
        (status = 503, description = "Configured DaaS upstream failed"),
    )
)]
pub async fn import_project_pdf(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    body: Result<Json<ImportPdfRequest>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ProjectPaper>)> {
    auth.require_scope(scope::PROJECTS_WRITE)?;
    project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Member).await?;
    let Json(req) = body.map_err(|_| crate::error::ApiError::invalid("invalid import body"))?;
    let paper = project_papers::import_pdf(&state.db, &state.daas, id, actor(&auth), req).await?;
    Ok((StatusCode::CREATED, Json(paper)))
}

#[utoipa::path(
    post, path = "/research-projects/{id}/search", tag = "projects",
    params(("id" = Uuid, Path, description = "Project id")),
    request_body = ProjectSearchRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, description = "Hybrid search outcome (stub provider returns an empty page)", body = ProjectSearchResponse),
        (status = 400, description = "Invalid query"),
        (status = 403, description = "Requires projects:read and Viewer; persist needs projects:write and Member"),
        (status = 409, description = "No active DaaS workspace binding"),
        (status = 503, description = "Configured DaaS upstream failed"),
    )
)]
pub async fn search_project(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    body: Result<Json<ProjectSearchRequest>, JsonRejection>,
) -> ApiResult<Json<ProjectSearchResponse>> {
    auth.require_scope(scope::PROJECTS_READ)?;
    let role = project_acl::require_project_role(&state.db, &auth, id, ProjectRole::Viewer).await?;
    let Json(req) = body.map_err(|_| crate::error::ApiError::invalid("invalid search body"))?;
    if req.persist {
        auth.require_scope(scope::PROJECTS_WRITE)?;
        if role < ProjectRole::Member {
            return Err(crate::error::ApiError::forbidden(
                "persist requires project role member or higher",
            ));
        }
    }
    Ok(Json(
        project_assets::search(&state.db, &state.daas, id, actor(&auth), req).await?,
    ))
}

/// OpenAPI shape of the shared pagination envelope.
#[derive(utoipa::ToSchema)]
#[allow(dead_code)]
struct PageProjectAsset {
    data: Vec<ProjectAsset>,
    next_cursor: Option<String>,
}
