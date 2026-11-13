//! HTTP handlers for scholar profiles.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::scholars::{self, PatchScholarRequest, Scholar},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    get, path = "/scholars", tag = "identity",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of scholar profiles"))
)]
pub async fn list_scholars(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Scholar>>> {
    auth.require_scope(scope::PAPERS_READ)?;
    Ok(Json(scholars::list(&state.db, &params).await?))
}

#[utoipa::path(
    get, path = "/scholars/{id}", tag = "identity",
    params(("id" = Uuid, Path, description = "Scholar id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, body = Scholar))
)]
pub async fn get_scholar(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Scholar>> {
    auth.require_scope(scope::PAPERS_READ)?;
    Ok(Json(scholars::get(&state.db, id).await?))
}

#[utoipa::path(
    patch, path = "/scholars/{id}", tag = "identity",
    params(("id" = Uuid, Path, description = "Scholar id")),
    request_body = PatchScholarRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, body = Scholar))
)]
pub async fn patch_scholar(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchScholarRequest>,
) -> ApiResult<Json<Scholar>> {
    // Owner check inside domain; any authenticated principal may attempt.
    Ok(Json(
        scholars::patch(&state.db, id, auth.owner_id(), req).await?,
    ))
}
