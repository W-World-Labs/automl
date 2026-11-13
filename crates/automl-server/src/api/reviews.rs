//! HTTP handlers for paper reviews.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::reviews::{self, CreateReviewRequest, Review},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/papers/{id}/reviews", tag = "reviews",
    params(("id" = Uuid, Path, description = "Paper id")),
    request_body = CreateReviewRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, body = Review))
)]
pub async fn create_review(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateReviewRequest>,
) -> ApiResult<(StatusCode, Json<Review>)> {
    auth.require_scope(scope::REVIEWS_WRITE)?;
    let review = reviews::create(&state.db, id, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(review)))
}

#[utoipa::path(
    get, path = "/papers/{id}/reviews", tag = "reviews",
    params(
        ("id" = Uuid, Path, description = "Paper id"),
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    responses((status = 200, description = "Page of reviews (public read)"))
)]
pub async fn list_reviews(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Review>>> {
    Ok(Json(reviews::list(&state.db, id, &params).await?))
}
