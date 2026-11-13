//! Admin surface for outbound webhook subscriptions + delivery inspection.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::events::{
        self, CreateSubscriptionRequest, Delivery, PatchSubscriptionRequest, Subscription,
    },
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/webhooks/subscriptions", tag = "webhooks",
    request_body = CreateSubscriptionRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, description = "Subscription created", body = Subscription))
)]
pub async fn create_subscription(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateSubscriptionRequest>,
) -> ApiResult<(StatusCode, Json<Subscription>)> {
    auth.require_scope(scope::ADMIN)?;
    let s = events::create_subscription(&state.db, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(s)))
}

#[utoipa::path(
    get, path = "/webhooks/subscriptions", tag = "webhooks",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of subscriptions"))
)]
pub async fn list_subscriptions(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Subscription>>> {
    auth.require_scope(scope::ADMIN)?;
    Ok(Json(events::list_subscriptions(&state.db, &params).await?))
}

#[utoipa::path(
    patch, path = "/webhooks/subscriptions/{id}", tag = "webhooks",
    params(("id" = Uuid, Path, description = "Subscription id")),
    request_body = PatchSubscriptionRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, body = Subscription))
)]
pub async fn patch_subscription(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchSubscriptionRequest>,
) -> ApiResult<Json<Subscription>> {
    auth.require_scope(scope::ADMIN)?;
    Ok(Json(events::patch_subscription(&state.db, id, req).await?))
}

#[utoipa::path(
    delete, path = "/webhooks/subscriptions/{id}", tag = "webhooks",
    params(("id" = Uuid, Path, description = "Subscription id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 204, description = "Deleted"))
)]
pub async fn delete_subscription(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::ADMIN)?;
    events::delete_subscription(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default, Deserialize)]
pub struct DeliveryListQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub status: Option<String>,
}

#[utoipa::path(
    get, path = "/webhooks/deliveries", tag = "webhooks",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
        ("status" = Option<String>, Query, description = "pending | delivered | dead"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of delivery attempts"))
)]
pub async fn list_deliveries(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(q): Query<DeliveryListQuery>,
) -> ApiResult<Json<Page<Delivery>>> {
    auth.require_scope(scope::ADMIN)?;
    let params = PageParams {
        limit: q.limit,
        cursor: q.cursor,
    };
    Ok(Json(
        events::list_deliveries(&state.db, &params, q.status.as_deref()).await?,
    ))
}
