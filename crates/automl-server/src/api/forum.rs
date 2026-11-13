//! HTTP handlers for the forum (categories → threads → posts, soft delete).

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
    domain::forum::{self, Category, CreatePostRequest, CreateThreadRequest, Post, Thread},
    error::{ApiError, ApiResult},
    state::AppState,
};

#[utoipa::path(
    get, path = "/forum/categories", tag = "community",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    responses((status = 200, description = "Page of categories (public read)"))
)]
pub async fn list_categories(
    State(state): State<AppState>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Category>>> {
    Ok(Json(forum::list_categories(&state.db, &params).await?))
}

#[derive(Debug, Default, Deserialize)]
pub struct ThreadListQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub category_id: Option<Uuid>,
}

#[utoipa::path(
    post, path = "/forum/threads", tag = "community",
    request_body = CreateThreadRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, description = "Thread created", body = Thread))
)]
pub async fn create_thread(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreateThreadRequest>,
) -> ApiResult<(StatusCode, Json<Thread>)> {
    auth.require_scope(scope::FORUM_WRITE)?;
    let t = forum::create_thread(&state.db, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(t)))
}

#[utoipa::path(
    get, path = "/forum/threads", tag = "community",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
        ("category_id" = Option<Uuid>, Query, description = "Filter by category"),
    ),
    responses((status = 200, description = "Page of threads (public read)"))
)]
pub async fn list_threads(
    State(state): State<AppState>,
    Query(q): Query<ThreadListQuery>,
) -> ApiResult<Json<Page<Thread>>> {
    let params = PageParams {
        limit: q.limit,
        cursor: q.cursor,
    };
    Ok(Json(
        forum::list_threads(&state.db, &params, q.category_id).await?,
    ))
}

#[utoipa::path(
    get, path = "/forum/threads/{id}", tag = "community",
    params(("id" = Uuid, Path, description = "Thread id")),
    responses((status = 200, body = Thread), (status = 404, description = "Missing or deleted"))
)]
pub async fn get_thread(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Thread>> {
    Ok(Json(forum::get_thread(&state.db, id).await?))
}

#[utoipa::path(
    delete, path = "/forum/threads/{id}", tag = "community",
    params(("id" = Uuid, Path, description = "Thread id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 204, description = "Soft-deleted (author or admin)"))
)]
pub async fn delete_thread(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::FORUM_WRITE)?;
    forum::delete_thread(&state.db, id, auth.owner_id(), auth.is_admin()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/forum/threads/{id}/posts", tag = "community",
    params(("id" = Uuid, Path, description = "Thread id")),
    request_body = CreatePostRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, description = "Post created", body = Post))
)]
pub async fn create_post(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<CreatePostRequest>,
) -> ApiResult<(StatusCode, Json<Post>)> {
    auth.require_scope(scope::FORUM_WRITE)?;
    let p = forum::create_post(&state.db, id, auth.owner_id(), req).await?;
    Ok((StatusCode::CREATED, Json(p)))
}

#[utoipa::path(
    get, path = "/forum/threads/{id}/posts", tag = "community",
    params(
        ("id" = Uuid, Path, description = "Thread id"),
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    responses((status = 200, description = "Page of posts, conversation order (public read)"))
)]
pub async fn list_posts(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Post>>> {
    Ok(Json(forum::list_posts(&state.db, id, &params).await?))
}

#[utoipa::path(
    delete, path = "/forum/posts/{id}", tag = "community",
    params(("id" = Uuid, Path, description = "Post id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 204, description = "Soft-deleted (author or admin)"))
)]
pub async fn delete_post(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    auth.require_scope(scope::FORUM_WRITE)?;
    forum::delete_post(&state.db, id, auth.owner_id(), auth.is_admin()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/notifications", tag = "community",
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 501, description = "Notification center is not implemented yet"))
)]
pub async fn notifications(_auth: AuthContext) -> ApiResult<Json<()>> {
    // Stub-is-501 rule: no fake empty inbox.
    Err(ApiError::not_implemented(
        "notification center is not implemented yet",
    ))
}
