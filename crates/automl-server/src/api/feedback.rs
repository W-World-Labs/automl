//! HTTP handlers for feedback. Submission is anonymous-friendly (no auth) and
//! sits behind the strictest rate bucket; triage is admin-only under /admin.

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
    domain::feedback::{self, CreateFeedbackRequest, Feedback, PatchFeedbackRequest},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/feedback", tag = "community",
    request_body = CreateFeedbackRequest,
    responses((status = 201, description = "Feedback recorded (anonymous allowed)", body = Feedback))
)]
pub async fn create_feedback(
    State(state): State<AppState>,
    Json(req): Json<CreateFeedbackRequest>,
) -> ApiResult<(StatusCode, Json<Feedback>)> {
    let f = feedback::create(&state.db, req).await?;
    Ok((StatusCode::CREATED, Json(f)))
}

#[derive(Debug, Default, Deserialize)]
pub struct FeedbackListQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub status: Option<String>,
}

#[utoipa::path(
    get, path = "/admin/feedback", tag = "community",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
        ("status" = Option<String>, Query, description = "new | triaged | closed"),
    ),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Page of feedback (admin)"))
)]
pub async fn list_feedback(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(q): Query<FeedbackListQuery>,
) -> ApiResult<Json<Page<Feedback>>> {
    auth.require_scope(scope::ADMIN)?;
    let params = PageParams {
        limit: q.limit,
        cursor: q.cursor,
    };
    Ok(Json(
        feedback::list(&state.db, &params, q.status.as_deref()).await?,
    ))
}

#[utoipa::path(
    patch, path = "/admin/feedback/{id}", tag = "community",
    params(("id" = Uuid, Path, description = "Feedback id")),
    request_body = PatchFeedbackRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Status updated", body = Feedback))
)]
pub async fn patch_feedback(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchFeedbackRequest>,
) -> ApiResult<Json<Feedback>> {
    auth.require_scope(scope::ADMIN)?;
    Ok(Json(feedback::set_status(&state.db, id, req).await?))
}
