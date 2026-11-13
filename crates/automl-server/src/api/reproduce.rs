//! HTTP handlers for the reproduce evidence chain.

use automl_core::scope;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

use crate::{
    auth::{AuthContext, Principal},
    domain::reproduce::{
        self, RegisterRunRequest, ReproduceRun, ReproductionSummary, RunEvidence,
        SubmitEvidenceRequest,
    },
    error::ApiResult,
    state::AppState,
};

fn runner_principal(auth: &AuthContext) -> String {
    match &auth.principal {
        Principal::Scholar { scholar_id, .. } => format!("scholar:{scholar_id}"),
        Principal::ApiKey { key_id, .. } => format!("key:{key_id}"),
    }
}

#[utoipa::path(
    post, path = "/reproduce/runs", tag = "reproduce",
    request_body = RegisterRunRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses(
        (status = 202, description = "Run registered; Location points at run resource", body = ReproduceRun),
    )
)]
pub async fn register_run(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(req): Json<RegisterRunRequest>,
) -> ApiResult<Response> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    let run = reproduce::register_run(&state.db, &runner_principal(&auth), req).await?;
    let location = format!("/api/v1/reproduce/runs/{}", run.id);
    Ok((
        StatusCode::ACCEPTED,
        [(http::header::LOCATION, location)],
        Json(run),
    )
        .into_response())
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct RunListQuery {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
    pub paper_id: Option<Uuid>,
}

#[utoipa::path(
    get, path = "/reproduce/runs", tag = "reproduce",
    params(
        ("limit" = Option<u32>, Query, description = "Page size"),
        ("cursor" = Option<String>, Query, description = "Keyset cursor"),
        ("paper_id" = Option<Uuid>, Query, description = "Filter by paper"),
    ),
    responses((status = 200, description = "Page of reproduce runs (public read)"))
)]
pub async fn list_runs(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<RunListQuery>,
) -> ApiResult<Json<automl_core::Page<ReproduceRun>>> {
    let params = automl_core::PageParams {
        limit: q.limit,
        cursor: q.cursor,
    };
    Ok(Json(
        reproduce::list_runs(&state.db, &params, q.paper_id).await?,
    ))
}

#[utoipa::path(
    get, path = "/reproduce/runs/{id}", tag = "reproduce",
    params(("id" = Uuid, Path, description = "Run id")),
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, body = ReproduceRun))
)]
pub async fn get_run(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ReproduceRun>> {
    auth.require_scope(scope::PAPERS_READ)?;
    Ok(Json(reproduce::get_run(&state.db, id).await?))
}

#[utoipa::path(
    put, path = "/reproduce/runs/{id}/evidence", tag = "reproduce",
    params(("id" = Uuid, Path, description = "Run id")),
    request_body = SubmitEvidenceRequest,
    security(("bearer_api_key" = []), ("session_cookie" = [])),
    responses((status = 200, description = "Evidence accepted or rejected on integrity"))
)]
pub async fn submit_evidence(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(id): Path<Uuid>,
    Json(req): Json<SubmitEvidenceRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    auth.require_scope(scope::REPRODUCE_SUBMIT)?;
    let (run, evidence) =
        reproduce::submit_evidence(&state.db, id, &runner_principal(&auth), req).await?;
    Ok(Json(serde_json::json!({
        "run": run,
        "evidence": evidence,
    })))
}

#[utoipa::path(
    get, path = "/papers/{id}/reproduction", tag = "reproduce",
    params(("id" = Uuid, Path, description = "Paper id")),
    responses((status = 200, body = ReproductionSummary))
)]
pub async fn paper_reproduction(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ReproductionSummary>> {
    Ok(Json(reproduce::reproduction_summary(&state.db, id).await?))
}

// Silence unused import if utoipa only references via schema
#[allow(dead_code)]
fn _schemas(_: RunEvidence) {}
