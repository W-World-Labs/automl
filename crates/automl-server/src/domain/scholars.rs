//! Scholar public profiles (iteration 4).

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Scholar {
    pub id: Uuid,
    pub w_id: String,
    pub display_name: String,
    pub affiliation: String,
    pub orcid: Option<String>,
    pub bio: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchScholarRequest {
    pub display_name: Option<String>,
    pub affiliation: Option<String>,
    pub orcid: Option<String>,
    pub bio: Option<String>,
}

pub async fn get(pool: &PgPool, id: Uuid) -> ApiResult<Scholar> {
    let row = sqlx::query_as::<_, ScholarRow>(
        r#"
        SELECT id, w_id, display_name, affiliation, orcid, bio, created_at, updated_at
        FROM scholars WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("scholar not found"))?;
    Ok(row.into())
}

pub async fn list(pool: &PgPool, params: &PageParams) -> ApiResult<Page<Scholar>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let rows = if let Some(cursor) = params.cursor.as_deref() {
        let (created_at, id) = super::cursor::decode(cursor)?;
        sqlx::query_as::<_, ScholarRow>(
            r#"
            SELECT id, w_id, display_name, affiliation, orcid, bio, created_at, updated_at
            FROM scholars
            WHERE (created_at, id) < ($1, $2)
            ORDER BY created_at DESC, id DESC
            LIMIT $3
            "#,
        )
        .bind(created_at)
        .bind(id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, ScholarRow>(
            r#"
            SELECT id, w_id, display_name, affiliation, orcid, bio, created_at, updated_at
            FROM scholars
            ORDER BY created_at DESC, id DESC
            LIMIT $1
            "#,
        )
        .bind(fetch)
        .fetch_all(pool)
        .await?
    };
    Ok(Page::from_rows(
        rows.into_iter().map(Scholar::from).collect(),
        limit,
        |s| super::cursor::encode(s.created_at, s.id),
    ))
}

pub async fn patch(
    pool: &PgPool,
    id: Uuid,
    owner_id: Uuid,
    req: PatchScholarRequest,
) -> ApiResult<Scholar> {
    if id != owner_id {
        return Err(ApiError::forbidden(
            "can only patch your own scholar profile",
        ));
    }
    let current = get(pool, id).await?;
    let display_name = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(current.display_name.as_str());
    let affiliation = req
        .affiliation
        .as_deref()
        .unwrap_or(current.affiliation.as_str());
    let orcid = req.orcid.or(current.orcid);
    if let Some(ref o) = orcid {
        validate_orcid(o)?;
    }
    let bio = req.bio.as_deref().unwrap_or(current.bio.as_str());

    let row = sqlx::query_as::<_, ScholarRow>(
        r#"
        UPDATE scholars SET
            display_name = $2,
            affiliation = $3,
            orcid = $4,
            bio = $5,
            updated_at = now()
        WHERE id = $1
        RETURNING id, w_id, display_name, affiliation, orcid, bio, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(display_name)
    .bind(affiliation)
    .bind(&orcid)
    .bind(bio)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

fn validate_orcid(o: &str) -> ApiResult<()> {
    // ORCID iD: 0000-0000-0000-000X (digits + optional trailing X)
    let ok = o.len() == 19
        && o.as_bytes().get(4) == Some(&b'-')
        && o.as_bytes().get(9) == Some(&b'-')
        && o.as_bytes().get(14) == Some(&b'-')
        && o.chars().enumerate().all(|(i, c)| match i {
            4 | 9 | 14 => c == '-',
            18 => c.is_ascii_digit() || c == 'X' || c == 'x',
            _ => c.is_ascii_digit(),
        });
    if ok {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "orcid must look like 0000-0000-0000-000X",
        ))
    }
}

#[derive(sqlx::FromRow)]
struct ScholarRow {
    id: Uuid,
    w_id: String,
    display_name: String,
    affiliation: String,
    orcid: Option<String>,
    bio: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ScholarRow> for Scholar {
    fn from(r: ScholarRow) -> Self {
        Self {
            id: r.id,
            w_id: r.w_id,
            display_name: r.display_name,
            affiliation: r.affiliation,
            orcid: r.orcid,
            bio: r.bio,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}
