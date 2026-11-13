//! HTTP handlers for templates + skills-manifest distribution (ETag/304).

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::templates::{self, CreateTemplateRequest, PatchTemplateRequest, Template},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/templates", tag = "templates",
    request_body = CreateTemplateRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, description = "Template created", body = Template))
)]
pub async fn create_template(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateTemplateRequest>,
) -> ApiResult<(StatusCode, Json<Template>)> {
    auth.require_scope(scope::ADMIN)?;
    let t = templates::create(&state.db, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(t)))
}

#[utoipa::path(
    get, path = "/templates", tag = "templates",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of templates"))
)]
pub async fn list_templates(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Template>>> {
    auth.require_scope(scope::PAPERS_READ)?;
    Ok(Json(templates::list(&state.db, &params).await?))
}

#[utoipa::path(
    get, path = "/templates/{id}", tag = "templates",
    params(("id" = Uuid, Path, description = "Template id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, body = Template))
)]
pub async fn get_template(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Template>> {
    auth.require_scope(scope::PAPERS_READ)?;
    Ok(Json(templates::get(&state.db, id).await?))
}

#[utoipa::path(
    patch, path = "/templates/{id}", tag = "templates",
    params(("id" = Uuid, Path, description = "Template id")),
    request_body = PatchTemplateRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Updated; version bumped", body = Template))
)]
pub async fn patch_template(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchTemplateRequest>,
) -> ApiResult<Json<Template>> {
    auth.require_scope(scope::ADMIN)?;
    Ok(Json(templates::patch(&state.db, id, req).await?))
}

#[utoipa::path(
    delete, path = "/templates/{id}", tag = "templates",
    params(("id" = Uuid, Path, description = "Template id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 204, description = "Deleted"))
)]
pub async fn delete_template(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::ADMIN)?;
    templates::delete(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/templates/{id}/manifest", tag = "templates",
    params(("id" = Uuid, Path, description = "Template id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, description = "Skills manifest; ETag is a content hash"),
        (status = 304, description = "Not modified (If-None-Match hit)"),
    )
)]
pub async fn get_manifest(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    auth.require_scope(scope::PAPERS_READ)?;
    let t = templates::get(&state.db, id).await?;
    let etag = t.etag();

    let if_none_match = headers
        .get(http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if if_none_match
        .split(',')
        .any(|c| c.trim().trim_start_matches("W/") == etag || c.trim() == "*")
    {
        return Ok((StatusCode::NOT_MODIFIED, [(http::header::ETAG, etag)]).into_response());
    }

    let body = serde_json::json!({
        "template_id": t.id,
        "name": t.name,
        "kind": t.kind,
        "version": t.version,
        "manifest": t.manifest,
    });
    Ok((StatusCode::OK, [(http::header::ETAG, etag)], Json(body)).into_response())
}
