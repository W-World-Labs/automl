//! HTTP handlers for papers.

use automl_core::{scope, Page, PageParams};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    auth::AuthContext,
    domain::papers::{self, CreatePaperRequest, Paper, PatchPaperRequest},
    error::ApiResult,
    state::AppState,
};

#[utoipa::path(
    post, path = "/papers", tag = "papers",
    request_body = CreatePaperRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 201, description = "Paper created", body = Paper))
)]
pub async fn create_paper(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<CreatePaperRequest>,
) -> ApiResult<(StatusCode, Json<Paper>)> {
    auth.require_scope(scope::PAPERS_WRITE)?;
    let paper = papers::create(&state.db, auth.owner_id(), req, state.gitea.is_some()).await?;

    // Platform-hosted: provision the Gitea repo off the request path. Failure
    // leaves code_status = repo_pending; the periodic sweep compensates.
    if paper.code_status == "repo_pending" {
        if let Some(client) = state.gitea.clone() {
            let db = state.db.clone();
            let paper_id = paper.id;
            tokio::spawn(async move {
                if let Err(err) =
                    crate::domain::gitea::provision_paper(&db, &client, paper_id).await
                {
                    tracing::warn!(%paper_id, error = %err, "initial gitea repo provisioning failed");
                }
            });
        }
    }
    Ok((StatusCode::CREATED, Json(paper)))
}

#[utoipa::path(
    get, path = "/papers", tag = "papers",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
    ),
    responses((status = 200, description = "Page of papers (public read)"))
)]
pub async fn list_papers(
    State(state): State<AppState>,
    Query(params): Query<PageParams>,
) -> ApiResult<Json<Page<Paper>>> {
    Ok(Json(papers::list(&state.db, &params).await?))
}

#[utoipa::path(
    get, path = "/papers/{id}", tag = "papers",
    params(("id" = Uuid, Path, description = "Paper id")),
    responses((status = 200, body = Paper))
)]
pub async fn get_paper(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Paper>> {
    Ok(Json(papers::get(&state.db, id).await?))
}

#[utoipa::path(
    patch, path = "/papers/{id}", tag = "papers",
    params(("id" = Uuid, Path, description = "Paper id")),
    request_body = PatchPaperRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 200, body = Paper),
        (status = 409, description = "Illegal review_status transition"),
    )
)]
pub async fn patch_paper(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchPaperRequest>,
) -> ApiResult<Json<Paper>> {
    auth.require_scope(scope::PAPERS_WRITE)?;
    // Human gate: authors submit/withdraw; moving into review or deciding is
    // an admin act (docs/STATE-MACHINES.md §1 triggers).
    if let Some(to) = req.review_status {
        use crate::domain::papers::ReviewStatus as RS;
        if matches!(to, RS::UnderReview | RS::Accepted | RS::Rejected) {
            auth.require_scope(scope::ADMIN)?;
        }
    }
    Ok(Json(papers::patch(&state.db, id, req).await?))
}
