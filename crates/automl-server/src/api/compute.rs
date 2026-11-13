//! HTTP handlers for client compute profiles (provenance only, no scheduling).

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::compute::{self, ComputeProfile, CreateProfileRequest, ProfileFilter},
    error::ApiResult,
    state::AppState,
};

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub agent_id: Option<Uuid>,
}

#[utoipa::path(
    post, path = "/compute/profiles", tag = "compute",
    request_body = CreateProfileRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 201, description = "Profile registered", body = ComputeProfile),
        (status = 200, description = "Identical profile already registered (idempotent reuse)", body = ComputeProfile),
    )
)]
pub async fn create_profile(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateProfileRequest>,
) -> ApiResult<(StatusCode, Json<ComputeProfile>)> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    let (profile, created) = compute::create(&state.db, auth.owner_id(), req).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(profile)))
}

#[utoipa::path(
    get, path = "/compute/profiles", tag = "compute",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
        ("agent_id" = Option<Uuid>, Query, description = "Filter by agent"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of compute profiles"))
)]
pub async fn list_profiles(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Page<ComputeProfile>>> {
    auth.require_scope(scope::PAPERS_READ)?;
    let params = PageParams {
        limit: q.limit,
        cursor: q.cursor,
    };
    let filter = ProfileFilter {
        agent_id: q.agent_id,
    };
    Ok(Json(compute::list(&state.db, &params, &filter).await?))
}
