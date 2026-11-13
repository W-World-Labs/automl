//! Paper reviews.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    Reject,
    Revise,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::Revise => "revise",
        }
    }

    fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "approve" => Ok(Self::Approve),
            "reject" => Ok(Self::Reject),
            "revise" => Ok(Self::Revise),
            _ => Err(ApiError::invalid(format!("unknown verdict: {s}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Review {
    pub id: Uuid,
    pub paper_id: Uuid,
    pub reviewer_id: Uuid,
    pub verdict: Verdict,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateReviewRequest {
    pub verdict: Verdict,
    #[serde(default)]
    pub body: String,
}

pub async fn create(
    pool: &PgPool,
    paper_id: Uuid,
    reviewer_id: Uuid,
    req: CreateReviewRequest,
) -> ApiResult<Review> {
    // Ensure paper exists.
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM papers WHERE id = $1)")
        .bind(paper_id)
        .fetch_one(pool)
        .await?;
    if !exists {
        return Err(ApiError::not_found("paper not found"));
    }

    let row = sqlx::query_as::<_, ReviewRow>(
        r#"
        INSERT INTO reviews (paper_id, reviewer_id, verdict, body)
        VALUES ($1, $2, $3, $4)
        RETURNING id, paper_id, reviewer_id, verdict, body, created_at
        "#,
    )
    .bind(paper_id)
    .bind(reviewer_id)
    .bind(req.verdict.as_str())
    .bind(&req.body)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn list(pool: &PgPool, paper_id: Uuid, params: &PageParams) -> ApiResult<Page<Review>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let rows = if let Some(cursor) = params.cursor.as_deref() {
        let (created_at, id) = super::cursor::decode(cursor)?;
        sqlx::query_as::<_, ReviewRow>(
            r#"
            SELECT id, paper_id, reviewer_id, verdict, body, created_at
            FROM reviews
            WHERE paper_id = $1 AND (created_at, id) < ($2, $3)
            ORDER BY created_at DESC, id DESC
            LIMIT $4
            "#,
        )
        .bind(paper_id)
        .bind(created_at)
        .bind(id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, ReviewRow>(
            r#"
            SELECT id, paper_id, reviewer_id, verdict, body, created_at
            FROM reviews
            WHERE paper_id = $1
            ORDER BY created_at DESC, id DESC
            LIMIT $2
            "#,
        )
        .bind(paper_id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    };
    Ok(Page::from_rows(
        rows.into_iter().map(Review::from).collect(),
        limit,
        |r| super::cursor::encode(r.created_at, r.id),
    ))
}

#[derive(sqlx::FromRow)]
struct ReviewRow {
    id: Uuid,
    paper_id: Uuid,
    reviewer_id: Uuid,
    verdict: String,
    body: String,
    created_at: DateTime<Utc>,
}

impl From<ReviewRow> for Review {
    fn from(r: ReviewRow) -> Self {
        Self {
            id: r.id,
            paper_id: r.paper_id,
            reviewer_id: r.reviewer_id,
            verdict: Verdict::parse(&r.verdict).unwrap_or(Verdict::Revise),
            body: r.body,
            created_at: r.created_at,
        }
    }
}
