//! HTTP handlers for API key CRUD.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    auth::{
        keys::{self, ApiKeyCreated, ApiKeyMeta, CreateKeyRequest},
        AuthContext,
    },
    error::ApiResult,
    state::AppState,
};

/// Create an API key. Plaintext is returned **once** in this response.
#[utoipa::path(
    post,
    path = "/keys",
    tag = "auth",
    request_body = CreateKeyRequest,
    security(
        ("bearer_api_key" = []),
        ("session_cookie" = [])
    ),
    responses(
        (status = 201, description = "Key created; plaintext returned once", body = ApiKeyCreated),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Missing keys:manage or grant scope"),
    )
)]
pub async fn create_key(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateKeyRequest>,
) -> ApiResult<(StatusCode, Json<ApiKeyCreated>)> {
    auth.require_scope(scope::KEYS_MANAGE)?;
    let created = keys::create_key(
        &state.db,
        auth.owner_id(),
        req,
        &auth.scopes_vec(),
        auth.is_admin(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// List the caller's non-revoked keys (metadata only).
#[utoipa::path(
    get,
    path = "/keys",
    tag = "auth",
    params(
        ("limit" = Option<u32>, Query, description = "Page size (1..=200)"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor from previous page"),
    ),
    security(
        ("bearer_api_key" = []),
        ("session_cookie" = [])
    ),
    responses(
        (status = 200, description = "Page of key metadata", body = PageApiKeyMeta),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Missing keys:manage"),
    )
)]
pub async fn list_keys(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<ApiKeyMeta>>> {
    auth.require_scope(scope::KEYS_MANAGE)?;
    let page = keys::list_keys(&state.db, auth.owner_id(), &params).await?;
    Ok(Json(page))
}

// utoipa needs a concrete type name for Page<ApiKeyMeta>.
#[allow(dead_code)]
#[derive(utoipa::ToSchema)]
struct PageApiKeyMeta {
    data: Vec<ApiKeyMeta>,
    next_cursor: Option<String>,
}

/// Revoke a key. Idempotent for already-revoked keys owned by the caller.
#[utoipa::path(
    delete,
    path = "/keys/{id}",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Key id")),
    security(
        ("bearer_api_key" = []),
        ("session_cookie" = [])
    ),
    responses(
        (status = 204, description = "Revoked (or already revoked)"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Missing keys:manage"),
        (status = 404, description = "Key not found"),
    )
)]
pub async fn revoke_key(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::KEYS_MANAGE)?;
    keys::revoke_key(&state.db, auth.owner_id(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}
