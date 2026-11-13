//! Research Hub project fuel shelf.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    daas::Daas,
    domain::projects::ActorRef,
    error::{ApiError, ApiResult},
    paper_enrichment::{PaperEnrichment, PaperIdentifier},
};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FuelRole {
    #[default]
    Seed,
    Related,
    Baseline,
    CitedBy,
    Reading,
}

impl FuelRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Related => "related",
            Self::Baseline => "baseline",
            Self::CitedBy => "cited_by",
            Self::Reading => "reading",
        }
    }
}

/// Supply exactly one nonempty primary identifier. Enrichment may discover
/// additional pointers for the stored row; the submitted identifier wins.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddProjectPaperRequest {
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub arxiv_id: Option<String>,
    #[serde(default)]
    pub w_papers_work_id: Option<String>,
    #[serde(default)]
    pub openalex_id: Option<String>,
    #[serde(default)]
    pub daas_document_id: Option<String>,
    #[serde(default)]
    pub fuel_role: FuelRole,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl AddProjectPaperRequest {
    pub fn identifier(&self) -> ApiResult<PaperIdentifier> {
        let values = [
            self.doi.as_deref().map(|s| ("doi", s)),
            self.arxiv_id.as_deref().map(|s| ("arxiv_id", s)),
            self.w_papers_work_id
                .as_deref()
                .map(|s| ("w_papers_work_id", s)),
            self.openalex_id.as_deref().map(|s| ("openalex_id", s)),
            self.daas_document_id
                .as_deref()
                .map(|s| ("daas_document_id", s)),
        ];
        let present: Vec<_> = values.into_iter().flatten().collect();
        if present.len() != 1 {
            return Err(ApiError::invalid(
                "exactly one primary paper identifier is required",
            ));
        }
        let (kind, value) = present[0];
        let value = value.trim();
        if value.is_empty() || matches!(value, "." | "..") || value.chars().any(char::is_control) {
            return Err(ApiError::invalid("paper identifier is empty or invalid"));
        }
        Ok(match kind {
            "doi" => {
                let doi = normalize_doi(value);
                if !valid_doi(&doi) {
                    return Err(ApiError::invalid(
                        "doi must be a DOI such as 10.1234/example",
                    ));
                }
                PaperIdentifier::Doi(doi)
            }
            "arxiv_id" => PaperIdentifier::Arxiv(value.to_string()),
            "w_papers_work_id" => PaperIdentifier::WPapers(value.to_string()),
            "openalex_id" => PaperIdentifier::OpenAlex(value.to_string()),
            _ => PaperIdentifier::DaasDocument(value.to_string()),
        })
    }
}

fn normalize_doi(value: &str) -> String {
    let mut doi = value.trim();
    let lower = doi.to_ascii_lowercase();
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
        "doi:",
    ] {
        if lower.starts_with(prefix) {
            doi = &doi[prefix.len()..];
            break;
        }
    }
    doi.trim().to_ascii_lowercase()
}

fn valid_doi(value: &str) -> bool {
    value
        .strip_prefix("10.")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(registrant, suffix)| {
            !registrant.is_empty()
                && registrant.bytes().all(|b| b.is_ascii_digit())
                && !suffix.is_empty()
                && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        })
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectPaper {
    pub id: Uuid,
    pub project_id: Uuid,
    pub fuel_role: FuelRole,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub openalex_id: Option<String>,
    pub w_papers_work_id: Option<String>,
    pub daas_document_id: Option<String>,
    pub paper_id: Option<Uuid>,
    pub notes: String,
    pub tags: Vec<String>,
    pub cached_title: Option<String>,
    pub cached_year: Option<i32>,
    pub cached_authors: Vec<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct PaperRow {
    id: Uuid,
    project_id: Uuid,
    fuel_role: String,
    doi: Option<String>,
    arxiv_id: Option<String>,
    openalex_id: Option<String>,
    w_papers_work_id: Option<String>,
    daas_document_id: Option<String>,
    paper_id: Option<Uuid>,
    notes: String,
    tags: Vec<String>,
    cached_title: Option<String>,
    cached_year: Option<i32>,
    cached_authors: serde_json::Value,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
impl From<PaperRow> for ProjectPaper {
    fn from(r: PaperRow) -> Self {
        Self {
            id: r.id,
            project_id: r.project_id,
            fuel_role: match r.fuel_role.as_str() {
                "related" => FuelRole::Related,
                "baseline" => FuelRole::Baseline,
                "cited_by" => FuelRole::CitedBy,
                "reading" => FuelRole::Reading,
                _ => FuelRole::Seed,
            },
            doi: r.doi,
            arxiv_id: r.arxiv_id,
            openalex_id: r.openalex_id,
            w_papers_work_id: r.w_papers_work_id,
            daas_document_id: r.daas_document_id,
            paper_id: r.paper_id,
            notes: r.notes,
            tags: r.tags,
            cached_title: r.cached_title,
            cached_year: r.cached_year,
            cached_authors: serde_json::from_value(r.cached_authors).unwrap_or_default(),
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub async fn add(
    pool: &PgPool,
    enrichment: &PaperEnrichment,
    project_id: Uuid,
    actor: ActorRef,
    req: AddProjectPaperRequest,
) -> ApiResult<ProjectPaper> {
    let identifier = req.identifier()?;
    if req.notes.contains('\0') || req.tags.iter().any(|tag| tag.contains('\0')) {
        return Err(ApiError::invalid("notes and tags must not contain NUL"));
    }
    let metadata = enrichment.enrich(&identifier).await;
    let (primary_doi, arxiv_id, primary_openalex, primary_wpapers, daas_document_id) =
        match &identifier {
            PaperIdentifier::Doi(s) => (Some(s.clone()), None, None, None, None),
            PaperIdentifier::Arxiv(s) => (None, Some(s.clone()), None, None, None),
            PaperIdentifier::WPapers(s) => (None, None, None, Some(s.clone()), None),
            PaperIdentifier::OpenAlex(s) => (None, None, Some(s.clone()), None, None),
            PaperIdentifier::DaasDocument(s) => (None, None, None, None, Some(s.clone())),
        };
    // Keep the submitted identifier authoritative; provider metadata may fill
    // other pointer/cache fields but cannot rewrite its identity.
    let doi = primary_doi.or_else(|| {
        metadata
            .doi
            .as_deref()
            .map(normalize_doi)
            .filter(|doi| valid_doi(doi))
    });
    let w_papers_work_id = if primary_wpapers.is_some() {
        primary_wpapers
    } else {
        metadata.w_papers_work_id.clone()
    };
    let openalex_id = if primary_openalex.is_some() {
        primary_openalex
    } else {
        metadata.openalex_id.clone()
    };
    let authors = serde_json::to_value(&metadata.authors).unwrap_or_else(|_| serde_json::json!([]));
    let notes = req.notes.trim();
    let tags: Vec<String> = req
        .tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let mut tx = pool.begin().await?;
    // Serialize against project deletion and membership changes, then recheck
    // member status inside the transaction before inserting shelf state.
    let live = sqlx::query_scalar::<_, bool>(
        "SELECT status <> 'deleted' FROM research_projects WHERE id=$1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(false);
    if !live {
        return Err(ApiError::not_found("project not found"));
    }
    let member = sqlx::query_scalar::<_, String>(
        "SELECT role FROM project_members
         WHERE project_id = $1 AND principal_type = 'scholar' AND scholar_id = $2
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(actor.scholar_id)
    .fetch_optional(&mut *tx)
    .await?;
    if !matches!(member.as_deref(), Some("owner" | "member")) {
        return Err(ApiError::forbidden(
            "requires project role member or higher",
        ));
    }
    let result = sqlx::query_as::<_, PaperRow>(
        r#"
        INSERT INTO project_papers (
            project_id, fuel_role, doi, arxiv_id, openalex_id, w_papers_work_id,
            daas_document_id, notes, tags, cached_title, cached_year, cached_authors, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        RETURNING id, project_id, fuel_role, doi, arxiv_id, openalex_id, w_papers_work_id,
                  daas_document_id, paper_id, notes, tags, cached_title, cached_year,
                  cached_authors, created_by, created_at, updated_at
        "#,
    )
    .bind(project_id)
    .bind(req.fuel_role.as_str())
    .bind(&doi)
    .bind(&arxiv_id)
    .bind(&openalex_id)
    .bind(&w_papers_work_id)
    .bind(&daas_document_id)
    .bind(notes)
    .bind(tags)
    .bind(
        metadata
            .title
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().to_string()),
    )
    .bind(metadata.year.filter(|y| (1000..=3000).contains(y)))
    .bind(authors)
    .bind(actor.scholar_id)
    .fetch_one(&mut *tx)
    .await;
    let row = match result {
        Ok(r) => r,
        Err(e) if matches!(&e, sqlx::Error::Database(db) if db.is_unique_violation()) => {
            return Err(ApiError::conflict(
                "this DOI is already on the project shelf",
            ))
        }
        Err(e) => return Err(e.into()),
    };
    sqlx::query(
        "INSERT INTO audit_events (project_id, actor_id, actor_kind, action, detail)
         VALUES ($1, $2, $3, 'paper.added', $4)",
    )
    .bind(project_id)
    .bind(actor.scholar_id)
    .bind(actor.kind)
    .bind(serde_json::json!({
        "project_paper_id": row.id,
        "identifier": {
            "doi": doi,
            "arxiv_id": arxiv_id,
            "w_papers_work_id": w_papers_work_id,
            "openalex_id": openalex_id,
            "daas_document_id": daas_document_id,
        }
    }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.into())
}

/// P2 PDF intake (design §12.2 path 4): the document registers as a DaaS
/// DocumentAsset inside the bound workspace, and the returned
/// `daas_document_id` lands on the fuel shelf via the normal add path —
/// member recheck and `paper.added` audit included.
///
/// With `StubDaaS` (no `DAAS_API_BASE` configured) the document id is a
/// synthetic `stub-doc-*` value. A caller that already holds a
/// `daas_document_id` can pass it directly and skip the upstream call.
/// A live upstream failure is an honest 503, never a fake id.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportPdfRequest {
    /// Human label for the document; sent to DaaS and cached as the shelf
    /// entry's display title when enrichment provides none.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional source URI the client already uploaded elsewhere.
    #[serde(default)]
    pub uri: Option<String>,
    /// Pre-registered DocumentAsset id — skips the upstream import call.
    #[serde(default)]
    pub daas_document_id: Option<String>,
    #[serde(default)]
    pub fuel_role: FuelRole,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn import_pdf(
    pool: &PgPool,
    daas: &Daas,
    project_id: Uuid,
    actor: ActorRef,
    req: ImportPdfRequest,
) -> ApiResult<ProjectPaper> {
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    if title.is_some_and(|t| t.len() > 512 || t.chars().any(char::is_control)) {
        return Err(ApiError::invalid(
            "title must be ≤512 chars without control characters",
        ));
    }
    let uri = req.uri.as_deref().map(str::trim).filter(|u| !u.is_empty());
    if uri.is_some_and(|u| u.len() > 1024 || u.chars().any(char::is_control)) {
        return Err(ApiError::invalid(
            "uri must be ≤1024 chars without control characters",
        ));
    }
    let daas_document_id = match req
        .daas_document_id
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        Some(id) => {
            if id.len() > 1024 || id.chars().any(char::is_control) {
                return Err(ApiError::invalid("daas_document_id is invalid"));
            }
            id.to_string()
        }
        None => {
            let workspace_id = super::project_bindings::active_workspace(pool, project_id)
                .await?
                .ok_or_else(|| {
                    ApiError::conflict(
                        "project has no active DaaS workspace binding; bind one first",
                    )
                })?;
            daas.import_document(&workspace_id, title.unwrap_or("document"), uri)
                .await?
                .document_id
        }
    };

    let mut paper = add(
        pool,
        // PDF rows gain nothing from paper metadata enrichment; the stub is
        // passed so this path never takes an unexpected upstream dependency.
        &PaperEnrichment::stub(),
        project_id,
        actor,
        AddProjectPaperRequest {
            doi: None,
            arxiv_id: None,
            w_papers_work_id: None,
            openalex_id: None,
            daas_document_id: Some(daas_document_id),
            fuel_role: req.fuel_role,
            notes: req.notes,
            tags: req.tags,
        },
    )
    .await?;

    if let Some(title) = title {
        if paper.cached_title.is_none() {
            sqlx::query(
                "UPDATE project_papers SET cached_title = $2, updated_at = now()
                 WHERE id = $1 AND cached_title IS NULL",
            )
            .bind(paper.id)
            .bind(title)
            .execute(pool)
            .await?;
            paper.cached_title = Some(title.to_string());
        }
    }
    Ok(paper)
}

pub async fn list(
    pool: &PgPool,
    project_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<ProjectPaper>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (ts, id) = match params.cursor.as_deref() {
        Some(c) => {
            let (t, i) = super::cursor::decode(c)?;
            (Some(t), Some(i))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, PaperRow>(
        r#"
        SELECT id, project_id, fuel_role, doi, arxiv_id, openalex_id, w_papers_work_id,
               daas_document_id, paper_id, notes, tags, cached_title, cached_year,
               cached_authors, created_by, created_at, updated_at
        FROM project_papers
        WHERE project_id = $1 AND deleted_at IS NULL
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(project_id)
    .bind(ts)
    .bind(id)
    .bind(fetch)
    .fetch_all(pool)
    .await?;
    Ok(Page::from_rows(
        rows.into_iter().map(ProjectPaper::from).collect(),
        limit,
        |p| super::cursor::encode(p.created_at, p.id),
    ))
}
