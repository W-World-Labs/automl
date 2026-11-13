//! HTTP handlers for agents and device bindings.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::agents::{self, Agent, AgentDevice, CreateAgentRequest, RegisterDeviceRequest},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/agents", tag = "identity",
    request_body = CreateAgentRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, body = Agent))
)]
pub async fn create_agent(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateAgentRequest>,
) -> ApiResult<(StatusCode, Json<Agent>)> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    let agent = agents::create(&state.db, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(agent)))
}

#[utoipa::path(
    get, path = "/agents", tag = "identity",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of caller's agents"))
)]
pub async fn list_agents(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Agent>>> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    Ok(Json(
        agents::list_for_owner(&state.db, auth.owner_id(), &params).await?,
    ))
}

#[utoipa::path(
    post, path = "/agents/{id}/devices", tag = "identity",
    params(("id" = Uuid, Path, description = "Agent id")),
    request_body = RegisterDeviceRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, body = AgentDevice))
)]
pub async fn register_device(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<RegisterDeviceRequest>,
) -> ApiResult<(StatusCode, Json<AgentDevice>)> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    let device = agents::register_device(&state.db, id, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(device)))
}

#[utoipa::path(
    delete, path = "/agents/{id}/devices/{device_id}", tag = "identity",
    params(
        ("id" = Uuid, Path, description = "Agent id"),
        ("device_id" = Uuid, Path, description = "Device id"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 204, description = "Revoked (idempotent)"))
)]
pub async fn revoke_device(
    State(state): State<AppState>,
    auth: AuthContext,
    Path((id, device_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    agents::revoke_device(&state.db, id, device_id, auth.owner_id()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/agents/{id}/devices", tag = "identity",
    params(("id" = Uuid, Path, description = "Agent id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Device list"))
)]
pub async fn list_devices(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<AgentDevice>>> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    Ok(Json(
        agents::list_devices(&state.db, id, auth.owner_id()).await?,
    ))
}
