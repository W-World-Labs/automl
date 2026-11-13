//! Papers: pointer-only records + review_status state machine.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Draft,
    Submitted,
    UnderReview,
    Accepted,
    Rejected,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Submitted => "submitted",
            Self::UnderReview => "under_review",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "draft" => Ok(Self::Draft),
            "submitted" => Ok(Self::Submitted),
            "under_review" => Ok(Self::UnderReview),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            _ => Err(ApiError::invalid(format!("unknown review_status: {s}"))),
        }
    }

    /// Legal transitions only. Terminal states cannot leave.
    pub fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Draft, Self::Submitted)
                | (Self::Submitted, Self::UnderReview)
                | (Self::Submitted, Self::Draft)
                | (Self::UnderReview, Self::Accepted)
                | (Self::UnderReview, Self::Rejected)
        )
    }
}

/// Platform-managed code hosting status. `unknown` is the terminal state for
/// external repo pointers; the platform-hosted chain is
/// `repo_pending → repo_ready → ready` (push observed). See
/// docs/STATE-MACHINES.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeStatus {
    Unknown,
    RepoPending,
    RepoReady,
    Ready,
}

impl CodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::RepoPending => "repo_pending",
            Self::RepoReady => "repo_ready",
            Self::Ready => "ready",
        }
    }

    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "unknown" => Ok(Self::Unknown),
            "repo_pending" => Ok(Self::RepoPending),
            "repo_ready" => Ok(Self::RepoReady),
            "ready" => Ok(Self::Ready),
            _ => Err(ApiError::invalid(format!("unknown code_status: {s}"))),
        }
    }

    /// Forward-only. `unknown` (external pointer) and `ready` are terminal;
    /// `repo_pending → ready` covers a push landing before the provision
    /// sweep flips `repo_ready`.
    pub fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::RepoPending, Self::RepoReady)
                | (Self::RepoPending, Self::Ready)
                | (Self::RepoReady, Self::Ready)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Paper {
    pub id: Uuid,
    pub title: String,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub authors: Value,
    pub repo_url: String,
    /// Author-pinned commit used for reproduction. `""` = not pinned yet
    /// (platform-hosted paper before the first pin).
    pub commit_sha: String,
    /// Last commit observed on the dedicated Gitea via push webhook.
    pub latest_commit: Option<String>,
    pub reproduce_path: String,
    pub code_status: String,
    pub review_status: ReviewStatus,
    pub submitted_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePaperRequest {
    pub title: String,
    #[serde(default, rename = "abstract")]
    pub abstract_text: String,
    #[serde(default = "empty_array")]
    pub authors: Value,
    /// External repo pointer. Omit to have the platform provision a repo on
    /// the dedicated Gitea instance (requires the integration to be configured).
    #[serde(default)]
    pub repo_url: Option<String>,
    /// Required alongside `repo_url`; omitted for platform-hosted papers
    /// (pinned later once code is pushed).
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default = "default_reproduce_path")]
    pub reproduce_path: String,
}

fn empty_array() -> Value {
    Value::Array(vec![])
}

fn default_reproduce_path() -> String {
    "reproduce.yaml".into()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchPaperRequest {
    pub title: Option<String>,
    #[serde(default, rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Option<Value>,
    pub repo_url: Option<String>,
    pub commit_sha: Option<String>,
    pub reproduce_path: Option<String>,
    pub code_status: Option<String>,
    pub review_status: Option<ReviewStatus>,
}

/// `platform_hosting` reflects whether the dedicated Gitea integration is
/// configured — only then may `repo_url` be omitted (the platform provisions
/// a repo; `code_status` starts at `repo_pending`).
pub async fn create(
    pool: &PgPool,
    owner: Uuid,
    req: CreatePaperRequest,
    platform_hosting: bool,
) -> ApiResult<Paper> {
    validate_create(&req, platform_hosting)?;
    let external_repo = req.repo_url.as_deref().map(str::trim).unwrap_or("");
    let commit_sha = req
        .commit_sha
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let code_status = if external_repo.is_empty() {
        "repo_pending"
    } else {
        "unknown"
    };
    let row = sqlx::query_as::<_, PaperRow>(
        r#"
        INSERT INTO papers (
            title, abstract, authors, repo_url, commit_sha,
            reproduce_path, code_status, submitted_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, title, abstract, authors, repo_url, commit_sha, latest_commit,
                  reproduce_path, code_status, review_status, submitted_by,
                  created_at, updated_at
        "#,
    )
    .bind(req.title.trim())
    .bind(&req.abstract_text)
    .bind(&req.authors)
    .bind(external_repo)
    .bind(commit_sha)
    .bind(req.reproduce_path.trim())
    .bind(code_status)
    .bind(owner)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn get(pool: &PgPool, id: Uuid) -> ApiResult<Paper> {
    let row = sqlx::query_as::<_, PaperRow>(
        r#"
        SELECT id, title, abstract, authors, repo_url, commit_sha, latest_commit,
               reproduce_path, code_status, review_status, submitted_by,
               created_at, updated_at
        FROM papers WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("paper not found"))?;
    Ok(row.into())
}

pub async fn list(pool: &PgPool, params: &PageParams) -> ApiResult<Page<Paper>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let rows = if let Some(cursor) = params.cursor.as_deref() {
        let (created_at, id) = super::cursor::decode(cursor)?;
        sqlx::query_as::<_, PaperRow>(
            r#"
            SELECT id, title, abstract, authors, repo_url, commit_sha, latest_commit,
                   reproduce_path, code_status, review_status, submitted_by,
                   created_at, updated_at
            FROM papers
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
        sqlx::query_as::<_, PaperRow>(
            r#"
            SELECT id, title, abstract, authors, repo_url, commit_sha, latest_commit,
                   reproduce_path, code_status, review_status, submitted_by,
                   created_at, updated_at
            FROM papers
            ORDER BY created_at DESC, id DESC
            LIMIT $1
            "#,
        )
        .bind(fetch)
        .fetch_all(pool)
        .await?
    };
    Ok(Page::from_rows(
        rows.into_iter().map(Paper::from).collect(),
        limit,
        |p| super::cursor::encode(p.created_at, p.id),
    ))
}

pub async fn patch(pool: &PgPool, id: Uuid, req: PatchPaperRequest) -> ApiResult<Paper> {
    let current = get(pool, id).await?;

    if let Some(to) = req.review_status {
        if to != current.review_status && !current.review_status.can_transition(to) {
            return Err(ApiError::conflict(format!(
                "illegal review_status transition: {} → {}",
                current.review_status.as_str(),
                to.as_str()
            )));
        }
    }

    if let Some(ref sha) = req.commit_sha {
        validate_commit_sha(sha)?;
    }
    if let Some(ref title) = req.title {
        if title.trim().is_empty() {
            return Err(ApiError::invalid("title must not be empty"));
        }
    }
    // code_status is platform-managed: a PATCH may only perform a legal
    // transition (and never invent values — enum + DB CHECK agree).
    if let Some(ref cs) = req.code_status {
        let to = CodeStatus::parse(cs)?;
        let from = CodeStatus::parse(&current.code_status)?;
        if to != from && !from.can_transition(to) {
            return Err(ApiError::conflict(format!(
                "illegal code_status transition: {} → {}",
                from.as_str(),
                to.as_str()
            )));
        }
    }

    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .unwrap_or(current.title.as_str());
    let abstract_text = req
        .abstract_text
        .as_deref()
        .unwrap_or(current.abstract_text.as_str());
    let authors = req.authors.as_ref().unwrap_or(&current.authors);
    let repo_url = req
        .repo_url
        .as_deref()
        .map(str::trim)
        .unwrap_or(current.repo_url.as_str());
    let commit_sha = req
        .commit_sha
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| current.commit_sha.clone());
    let reproduce_path = req
        .reproduce_path
        .as_deref()
        .map(str::trim)
        .unwrap_or(current.reproduce_path.as_str());
    let code_status = req
        .code_status
        .as_deref()
        .unwrap_or(current.code_status.as_str());
    let review_status = req.review_status.unwrap_or(current.review_status);

    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, PaperRow>(
        r#"
        UPDATE papers SET
            title = $2,
            abstract = $3,
            authors = $4,
            repo_url = $5,
            commit_sha = $6,
            reproduce_path = $7,
            code_status = $8,
            review_status = $9,
            updated_at = now()
        WHERE id = $1
        RETURNING id, title, abstract, authors, repo_url, commit_sha, latest_commit,
                  reproduce_path, code_status, review_status, submitted_by,
                  created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(title)
    .bind(abstract_text)
    .bind(authors)
    .bind(repo_url)
    .bind(commit_sha)
    .bind(reproduce_path)
    .bind(code_status)
    .bind(review_status.as_str())
    .fetch_one(&mut *tx)
    .await?;
    let paper: Paper = row.into();

    // Human-gate decision is an outbound event; the outbox row commits (or
    // rolls back) atomically with the decision itself.
    let decided = review_status != current.review_status
        && matches!(
            review_status,
            ReviewStatus::Accepted | ReviewStatus::Rejected
        );
    if decided {
        super::events::emit(
            &mut tx,
            automl_core::event::PAPER_DECIDED,
            serde_json::json!({
                "paper_id": paper.id,
                "decision": paper.review_status.as_str(),
                "title": paper.title,
                "submitted_by": paper.submitted_by,
            }),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(paper)
}

fn validate_create(req: &CreatePaperRequest, platform_hosting: bool) -> ApiResult<()> {
    if req.title.trim().is_empty() {
        return Err(ApiError::invalid("title must not be empty"));
    }
    let has_repo = req
        .repo_url
        .as_deref()
        .is_some_and(|u| !u.trim().is_empty());
    if has_repo {
        // External pointer must pin the commit it refers to.
        match req.commit_sha.as_deref() {
            Some(sha) => validate_commit_sha(sha)?,
            None => {
                return Err(ApiError::invalid(
                    "commit_sha is required when repo_url is provided",
                ))
            }
        }
    } else {
        if !platform_hosting {
            return Err(ApiError::invalid(
                "repo_url is required (platform code hosting is not configured)",
            ));
        }
        if let Some(sha) = req.commit_sha.as_deref() {
            validate_commit_sha(sha)?;
        }
    }
    if req.reproduce_path.trim().is_empty() {
        return Err(ApiError::invalid("reproduce_path must not be empty"));
    }
    if !req.authors.is_array() {
        return Err(ApiError::invalid("authors must be a JSON array"));
    }
    Ok(())
}

fn validate_commit_sha(sha: &str) -> ApiResult<()> {
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::invalid(
            "commit_sha must be 40 lowercase hex characters",
        ));
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct PaperRow {
    id: Uuid,
    title: String,
    #[sqlx(rename = "abstract")]
    abstract_text: String,
    authors: Value,
    repo_url: String,
    commit_sha: String,
    latest_commit: Option<String>,
    reproduce_path: String,
    code_status: String,
    review_status: String,
    submitted_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PaperRow> for Paper {
    fn from(r: PaperRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            abstract_text: r.abstract_text,
            authors: r.authors,
            repo_url: r.repo_url,
            commit_sha: r.commit_sha.trim().to_string(),
            latest_commit: r.latest_commit,
            reproduce_path: r.reproduce_path,
            code_status: r.code_status,
            review_status: ReviewStatus::parse(&r.review_status).unwrap_or(ReviewStatus::Draft),
            submitted_by: r.submitted_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_status_machine() {
        use CodeStatus::*;
        assert!(RepoPending.can_transition(RepoReady));
        assert!(RepoPending.can_transition(Ready));
        assert!(RepoReady.can_transition(Ready));
        // Terminals never move.
        for s in [Unknown, Ready] {
            for t in [Unknown, RepoPending, RepoReady, Ready] {
                assert!(!s.can_transition(t), "{s:?} -> {t:?} must be illegal");
            }
        }
        // Never backwards.
        assert!(!RepoReady.can_transition(RepoPending));
        assert!(!Ready.can_transition(RepoReady));
        assert!(CodeStatus::parse("bogus").is_err());
    }

    #[test]
    fn state_machine_allows_happy_path_only() {
        assert!(ReviewStatus::Draft.can_transition(ReviewStatus::Submitted));
        assert!(ReviewStatus::Submitted.can_transition(ReviewStatus::UnderReview));
        assert!(ReviewStatus::UnderReview.can_transition(ReviewStatus::Accepted));
        assert!(ReviewStatus::UnderReview.can_transition(ReviewStatus::Rejected));
        assert!(!ReviewStatus::Draft.can_transition(ReviewStatus::Accepted));
        assert!(!ReviewStatus::Accepted.can_transition(ReviewStatus::Draft));
        assert!(!ReviewStatus::Rejected.can_transition(ReviewStatus::UnderReview));
    }
}
