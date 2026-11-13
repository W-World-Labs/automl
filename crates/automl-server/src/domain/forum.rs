//! Forum: seeded categories → threads → posts. Markdown is stored raw and
//! rendered client-side only. Deletion is soft (`deleted_at`): lists filter it,
//! direct fetch of a deleted resource is 404.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Category {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Thread {
    pub id: Uuid,
    pub category_id: Uuid,
    pub title: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Post {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub body: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateThreadRequest {
    pub category_id: Uuid,
    pub title: String,
    /// Optional first post (markdown), created atomically with the thread.
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePostRequest {
    pub body: String,
}

pub async fn list_categories(db: &PgPool, params: &PageParams) -> ApiResult<Page<Category>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, CategoryRow>(
        r#"
        SELECT id, slug, name, description, created_at
        FROM forum_categories
        WHERE ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
        ORDER BY created_at DESC, id DESC
        LIMIT $3
        "#,
    )
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(db)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(Category::from).collect(),
        limit,
        |c| super::cursor::encode(c.created_at, c.id),
    ))
}

pub async fn create_thread(
    db: &PgPool,
    author: Uuid,
    req: CreateThreadRequest,
) -> ApiResult<Thread> {
    if req.title.trim().is_empty() {
        return Err(ApiError::invalid("title must not be empty"));
    }
    if let Some(ref body) = req.body {
        if body.trim().is_empty() {
            return Err(ApiError::invalid("body must not be empty when provided"));
        }
    }
    let cat: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM forum_categories WHERE id = $1")
        .bind(req.category_id)
        .fetch_optional(db)
        .await?;
    if cat.is_none() {
        return Err(ApiError::invalid("unknown category_id"));
    }

    let mut tx = db.begin().await?;
    let row = sqlx::query_as::<_, ThreadRow>(
        r#"
        INSERT INTO forum_threads (category_id, title, created_by)
        VALUES ($1, $2, $3)
        RETURNING id, category_id, title, created_by, created_at, updated_at
        "#,
    )
    .bind(req.category_id)
    .bind(req.title.trim())
    .bind(author)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(body) = req.body.as_deref() {
        sqlx::query("INSERT INTO forum_posts (thread_id, body, created_by) VALUES ($1, $2, $3)")
            .bind(row.id)
            .bind(body)
            .bind(author)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(row.into())
}

pub async fn get_thread(db: &PgPool, id: Uuid) -> ApiResult<Thread> {
    let row = sqlx::query_as::<_, ThreadRow>(
        r#"
        SELECT id, category_id, title, created_by, created_at, updated_at
        FROM forum_threads WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| ApiError::not_found("thread not found"))?;
    Ok(row.into())
}

pub async fn list_threads(
    db: &PgPool,
    params: &PageParams,
    category_id: Option<Uuid>,
) -> ApiResult<Page<Thread>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, ThreadRow>(
        r#"
        SELECT id, category_id, title, created_by, created_at, updated_at
        FROM forum_threads
        WHERE deleted_at IS NULL
          AND ($1::uuid IS NULL OR category_id = $1)
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(category_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(db)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(Thread::from).collect(),
        limit,
        |t| super::cursor::encode(t.created_at, t.id),
    ))
}

/// Soft-delete; only the author or an admin may delete.
pub async fn delete_thread(db: &PgPool, id: Uuid, actor: Uuid, is_admin: bool) -> ApiResult<()> {
    let thread = get_thread(db, id).await?;
    if thread.created_by != actor && !is_admin {
        return Err(ApiError::forbidden("only the author or admin may delete"));
    }
    sqlx::query("UPDATE forum_threads SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn create_post(
    db: &PgPool,
    thread_id: Uuid,
    author: Uuid,
    req: CreatePostRequest,
) -> ApiResult<Post> {
    if req.body.trim().is_empty() {
        return Err(ApiError::invalid("body must not be empty"));
    }
    // Posting into a deleted/missing thread is a 404.
    get_thread(db, thread_id).await?;
    let row = sqlx::query_as::<_, PostRow>(
        r#"
        INSERT INTO forum_posts (thread_id, body, created_by)
        VALUES ($1, $2, $3)
        RETURNING id, thread_id, body, created_by, created_at
        "#,
    )
    .bind(thread_id)
    .bind(&req.body)
    .bind(author)
    .fetch_one(db)
    .await?;
    sqlx::query("UPDATE forum_threads SET updated_at = now() WHERE id = $1")
        .bind(thread_id)
        .execute(db)
        .await?;
    Ok(row.into())
}

/// Posts read oldest-first (conversation order).
pub async fn list_posts(
    db: &PgPool,
    thread_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<Post>> {
    get_thread(db, thread_id).await?;
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, PostRow>(
        r#"
        SELECT id, thread_id, body, created_by, created_at
        FROM forum_posts
        WHERE thread_id = $1 AND deleted_at IS NULL
          AND ($2::timestamptz IS NULL OR (created_at, id) > ($2, $3))
        ORDER BY created_at ASC, id ASC
        LIMIT $4
        "#,
    )
    .bind(thread_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(db)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(Post::from).collect(),
        limit,
        |p| super::cursor::encode(p.created_at, p.id),
    ))
}

/// Soft-delete a post; author or admin only. Deleted posts vanish from lists.
pub async fn delete_post(db: &PgPool, id: Uuid, actor: Uuid, is_admin: bool) -> ApiResult<()> {
    let row: Option<(Uuid,)> =
        sqlx::query_as("SELECT created_by FROM forum_posts WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(db)
            .await?;
    let (created_by,) = row.ok_or_else(|| ApiError::not_found("post not found"))?;
    if created_by != actor && !is_admin {
        return Err(ApiError::forbidden("only the author or admin may delete"));
    }
    sqlx::query("UPDATE forum_posts SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct CategoryRow {
    id: Uuid,
    slug: String,
    name: String,
    description: String,
    created_at: DateTime<Utc>,
}

impl From<CategoryRow> for Category {
    fn from(r: CategoryRow) -> Self {
        Self {
            id: r.id,
            slug: r.slug,
            name: r.name,
            description: r.description,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ThreadRow {
    id: Uuid,
    category_id: Uuid,
    title: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ThreadRow> for Thread {
    fn from(r: ThreadRow) -> Self {
        Self {
            id: r.id,
            category_id: r.category_id,
            title: r.title,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct PostRow {
    id: Uuid,
    thread_id: Uuid,
    body: String,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<PostRow> for Post {
    fn from(r: PostRow) -> Self {
        Self {
            id: r.id,
            thread_id: r.thread_id,
            body: r.body,
            created_by: r.created_by,
            created_at: r.created_at,
        }
    }
}
