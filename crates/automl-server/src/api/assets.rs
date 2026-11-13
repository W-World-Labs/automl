//! HTTP handlers for presigned asset upload + commit.

use automl_core::scope;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::assets::{self, Asset, AssetCreated, CreateAssetRequest},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/papers/{id}/assets", tag = "assets",
    params(("id" = Uuid, Path, description = "Paper id")),
    request_body = CreateAssetRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, body = AssetCreated))
)]
pub async fn create_asset(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateAssetRequest>,
) -> ApiResult<(StatusCode, Json<AssetCreated>)> {
    auth.require_scope(scope::ASSETS_WRITE)?;
    let created =
        assets::create_pending(&state.db, &state.storage, id, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[utoipa::path(
    post, path = "/assets/{id}/commit", tag = "assets",
    params(("id" = Uuid, Path, description = "Asset id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, body = Asset),
        (status = 409, description = "Object missing or sha256 mismatch"),
    )
)]
pub async fn commit_asset(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Asset>> {
    auth.require_scope(scope::ASSETS_WRITE)?;
    Ok(Json(assets::commit(&state.db, &state.storage, id).await?))
}

#[utoipa::path(
    get, path = "/assets/{id}", tag = "assets",
    params(("id" = Uuid, Path, description = "Asset id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, body = Asset))
)]
pub async fn get_asset(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Asset>> {
    auth.require_scope(scope::PAPERS_READ)?;
    Ok(Json(assets::get(&state.db, id).await?))
}
