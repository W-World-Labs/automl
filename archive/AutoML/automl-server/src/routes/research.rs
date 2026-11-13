// ============================================================
// Research graph routes — agendas, hypotheses, skill runs,
// debates, and agent capability attestations.
// ============================================================

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agendas", get(list_agendas).post(create_agenda))
        .route("/agendas/{id}", get(get_agenda))
        .route("/agendas/{id}/hypotheses", post(create_hypothesis))
        .route("/hypotheses/{id}/skill-runs", post(create_skill_run))
        .route("/hypotheses/{id}/debates", post(create_debate))
        .route("/agents/{id}/capabilities", post(create_capability))
        .route("/agents/{id}/lab-profile", get(get_lab_profile))
}

async fn list_agendas(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<(
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT id, title, scope, owner_user_id, workspace_id, status, created_at \
             FROM research_agendas ORDER BY created_at DESC LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;

    let agendas: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, title, scope, owner_user_id, workspace_id, status, created_at)| {
                json!({
                    "id": id,
                    "title": title,
                    "scope": scope,
                    "owner_user_id": owner_user_id,
                    "workspace_id": workspace_id,
                    "status": status,
                    "created_at": created_at,
                })
            },
        )
        .collect();

    Ok(Json(json!({ "agendas": agendas, "total": agendas.len() })))
}

#[derive(Debug, Deserialize)]
struct CreateAgenda {
    title: String,
    scope: Option<String>,
    workspace_id: Option<String>,
    status: Option<String>,
}

async fn create_agenda(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateAgenda>,
) -> ApiResult<Json<Value>> {
    if body.title.trim().is_empty() {
        return Err(ApiError::bad_request("title is required"));
    }

    let id = format!("agenda_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let workspace_id = body.workspace_id.or(user.workspace_id);
    let status = body.status.unwrap_or_else(|| "active".to_string());

    sqlx::query(
        "INSERT INTO research_agendas \
         (id, title, scope, owner_user_id, workspace_id, status, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP::text, CURRENT_TIMESTAMP::text)",
    )
    .bind(&id)
    .bind(&body.title)
    .bind(body.scope.as_deref().unwrap_or(""))
    .bind(&user.owner_user_id)
    .bind(&workspace_id)
    .bind(&status)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "agenda": {
            "id": id,
            "title": body.title,
            "scope": body.scope.unwrap_or_default(),
            "owner_user_id": user.owner_user_id,
            "workspace_id": workspace_id,
            "status": status,
        }
    })))
}

async fn get_agenda(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let agenda: Option<(
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT id, title, scope, owner_user_id, workspace_id, status, created_at \
             FROM research_agendas WHERE id = $1",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    let Some((id, title, scope, owner_user_id, workspace_id, status, created_at)) = agenda else {
        return Err(ApiError::not_found("Agenda not found"));
    };

    let hypotheses: Vec<(
        String,
        Option<String>,
        String,
        Option<f64>,
        String,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT id, parent_hypothesis_id, claim, novelty_score, status, created_by, created_at \
             FROM hypotheses WHERE agenda_id = $1 ORDER BY created_at ASC LIMIT 200",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;

    let hypotheses: Vec<Value> = hypotheses
        .into_iter()
        .map(
            |(id, parent_hypothesis_id, claim, novelty_score, status, created_by, created_at)| {
                json!({
                    "id": id,
                    "parent_hypothesis_id": parent_hypothesis_id,
                    "claim": claim,
                    "novelty_score": novelty_score,
                    "status": status,
                    "created_by": created_by,
                    "created_at": created_at,
                })
            },
        )
        .collect();

    Ok(Json(json!({
        "agenda": {
            "id": id,
            "title": title,
            "scope": scope,
            "owner_user_id": owner_user_id,
            "workspace_id": workspace_id,
            "status": status,
            "created_at": created_at,
        },
        "hypotheses": hypotheses,
    })))
}

#[derive(Debug, Deserialize)]
struct CreateHypothesis {
    claim: String,
    parent_hypothesis_id: Option<String>,
    novelty_score: Option<f64>,
    status: Option<String>,
}

async fn create_hypothesis(
    State(state): State<AppState>,
    user: AuthUser,
    Path(agenda_id): Path<String>,
    Json(body): Json<CreateHypothesis>,
) -> ApiResult<Json<Value>> {
    if body.claim.trim().is_empty() {
        return Err(ApiError::bad_request("claim is required"));
    }

    ensure_exists(&state, "research_agendas", &agenda_id, "Agenda not found").await?;
    if let Some(parent_id) = body.parent_hypothesis_id.as_ref() {
        ensure_exists(
            &state,
            "hypotheses",
            parent_id,
            "Parent hypothesis not found",
        )
        .await?;
    }

    let id = format!("hyp_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let status = body.status.unwrap_or_else(|| "proposed".to_string());

    sqlx::query(
        "INSERT INTO hypotheses \
         (id, agenda_id, parent_hypothesis_id, claim, novelty_score, status, created_by, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP::text, CURRENT_TIMESTAMP::text)",
    )
    .bind(&id)
    .bind(&agenda_id)
    .bind(&body.parent_hypothesis_id)
    .bind(&body.claim)
    .bind(body.novelty_score)
    .bind(&status)
    .bind(&user.scholar_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "hypothesis": {
            "id": id,
            "agenda_id": agenda_id,
            "parent_hypothesis_id": body.parent_hypothesis_id,
            "claim": body.claim,
            "novelty_score": body.novelty_score,
            "status": status,
            "created_by": user.scholar_id,
        }
    })))
}

#[derive(Debug, Deserialize)]
struct CreateSkillRun {
    skill_id: String,
    agent_id: Option<String>,
    runtime: Option<String>,
    inputs_ref: Option<String>,
    outputs_ref: Option<String>,
    logs_ref: Option<String>,
    status: Option<String>,
}

async fn create_skill_run(
    State(state): State<AppState>,
    user: AuthUser,
    Path(hypothesis_id): Path<String>,
    Json(body): Json<CreateSkillRun>,
) -> ApiResult<Json<Value>> {
    if body.skill_id.trim().is_empty() {
        return Err(ApiError::bad_request("skill_id is required"));
    }
    ensure_exists(&state, "hypotheses", &hypothesis_id, "Hypothesis not found").await?;

    let id = format!("sr_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let agent_id = body.agent_id.unwrap_or_else(|| user.scholar_id.clone());
    let status = body.status.unwrap_or_else(|| "submitted".to_string());

    sqlx::query(
        "INSERT INTO skill_runs \
         (id, hypothesis_id, skill_id, agent_id, runtime, inputs_ref, outputs_ref, logs_ref, status, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, CURRENT_TIMESTAMP::text)",
    )
    .bind(&id)
    .bind(&hypothesis_id)
    .bind(&body.skill_id)
    .bind(&agent_id)
    .bind(body.runtime.as_deref().unwrap_or(""))
    .bind(body.inputs_ref.as_deref().unwrap_or(""))
    .bind(body.outputs_ref.as_deref().unwrap_or(""))
    .bind(body.logs_ref.as_deref().unwrap_or(""))
    .bind(&status)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "skill_run": {
            "id": id,
            "hypothesis_id": hypothesis_id,
            "skill_id": body.skill_id,
            "agent_id": agent_id,
            "runtime": body.runtime.unwrap_or_default(),
            "inputs_ref": body.inputs_ref.unwrap_or_default(),
            "outputs_ref": body.outputs_ref.unwrap_or_default(),
            "logs_ref": body.logs_ref.unwrap_or_default(),
            "status": status,
        }
    })))
}

#[derive(Debug, Deserialize)]
struct CreateDebate {
    participants: Option<Value>,
    transcript_ref: Option<String>,
    verdict: Option<String>,
}

async fn create_debate(
    State(state): State<AppState>,
    user: AuthUser,
    Path(hypothesis_id): Path<String>,
    Json(body): Json<CreateDebate>,
) -> ApiResult<Json<Value>> {
    ensure_exists(&state, "hypotheses", &hypothesis_id, "Hypothesis not found").await?;

    let id = format!("debate_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let participants = body
        .participants
        .unwrap_or_else(|| json!([{ "scholar_id": user.scholar_id }]));
    let participants_json = serde_json::to_string(&participants).unwrap_or_else(|_| "[]".into());
    let verdict = body.verdict.unwrap_or_default();

    sqlx::query(
        "INSERT INTO research_debates \
         (id, target_type, target_id, participants_json, transcript_ref, verdict, created_by, created_at) \
         VALUES ($1, 'hypothesis', $2, $3, $4, $5, $6, CURRENT_TIMESTAMP::text)",
    )
    .bind(&id)
    .bind(&hypothesis_id)
    .bind(&participants_json)
    .bind(body.transcript_ref.as_deref().unwrap_or(""))
    .bind(&verdict)
    .bind(&user.scholar_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "debate": {
            "id": id,
            "target_type": "hypothesis",
            "target_id": hypothesis_id,
            "participants": participants,
            "transcript_ref": body.transcript_ref.unwrap_or_default(),
            "verdict": verdict,
            "created_by": user.scholar_id,
        }
    })))
}

#[derive(Debug, Deserialize)]
struct CapabilityRequest {
    radar_project_id: Option<String>,
    runtime: Option<String>,
    model_or_engine: Option<String>,
    tooling_surface: Option<String>,
    supported_skills: Option<Value>,
    benchmark_score: Option<Value>,
    evidence_policy: Option<String>,
}

async fn create_capability(
    State(state): State<AppState>,
    user: AuthUser,
    Path(agent_id): Path<String>,
    Json(body): Json<CapabilityRequest>,
) -> ApiResult<Json<Value>> {
    ensure_exists(&state, "scholars", &agent_id, "Agent not found").await?;

    let id = format!("cap_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let supported_skills = body.supported_skills.unwrap_or_else(|| json!([]));
    let benchmark_score = body.benchmark_score.unwrap_or_else(|| json!({}));
    let supported_skills_json =
        serde_json::to_string(&supported_skills).unwrap_or_else(|_| "[]".into());
    let benchmark_score_json =
        serde_json::to_string(&benchmark_score).unwrap_or_else(|_| "{}".into());

    sqlx::query(
        "INSERT INTO capability_attestations \
         (id, agent_id, radar_project_id, runtime, model_or_engine, tooling_surface, supported_skills_json, benchmark_score_json, evidence_policy, attested_by, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, CURRENT_TIMESTAMP::text)",
    )
    .bind(&id)
    .bind(&agent_id)
    .bind(body.radar_project_id.as_deref().unwrap_or(""))
    .bind(body.runtime.as_deref().unwrap_or(""))
    .bind(body.model_or_engine.as_deref().unwrap_or(""))
    .bind(body.tooling_surface.as_deref().unwrap_or(""))
    .bind(&supported_skills_json)
    .bind(&benchmark_score_json)
    .bind(body.evidence_policy.as_deref().unwrap_or(""))
    .bind(&user.scholar_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "capability": {
            "id": id,
            "agent_id": agent_id,
            "radar_project_id": body.radar_project_id.unwrap_or_default(),
            "runtime": body.runtime.unwrap_or_default(),
            "model_or_engine": body.model_or_engine.unwrap_or_default(),
            "tooling_surface": body.tooling_surface.unwrap_or_default(),
            "supported_skills": supported_skills,
            "benchmark_score": benchmark_score,
            "evidence_policy": body.evidence_policy.unwrap_or_default(),
            "attested_by": user.scholar_id,
        }
    })))
}

async fn get_lab_profile(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let scholar: Option<(String, String, String, String)> =
        sqlx::query_as("SELECT id, name, type, joined_at FROM scholars WHERE id = $1")
            .bind(&agent_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((id, name, scholar_type, joined_at)) = scholar else {
        return Err(ApiError::not_found("Agent not found"));
    };

    let capabilities: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = sqlx::query_as(
        "SELECT id, radar_project_id, runtime, model_or_engine, tooling_surface, \
             supported_skills_json, benchmark_score_json, evidence_policy, created_at \
             FROM capability_attestations WHERE agent_id = $1 ORDER BY created_at DESC LIMIT 20",
    )
    .bind(&agent_id)
    .fetch_all(&state.db)
    .await?;

    let capabilities: Vec<Value> = capabilities
        .into_iter()
        .map(
            |(
                id,
                radar_project_id,
                runtime,
                model_or_engine,
                tooling_surface,
                supported_skills_json,
                benchmark_score_json,
                evidence_policy,
                created_at,
            )| {
                json!({
                    "id": id,
                    "radar_project_id": radar_project_id,
                    "runtime": runtime,
                    "model_or_engine": model_or_engine,
                    "tooling_surface": tooling_surface,
                    "supported_skills": serde_json::from_str::<Value>(&supported_skills_json).unwrap_or(json!([])),
                    "benchmark_score": serde_json::from_str::<Value>(&benchmark_score_json).unwrap_or(json!({})),
                    "evidence_policy": evidence_policy,
                    "created_at": created_at,
                })
            },
        )
        .collect();

    let skill_runs = count_for(&state, "skill_runs", "agent_id", &agent_id).await?;
    let reviews = count_for(&state, "reviews", "reviewer_id", &agent_id).await?;

    Ok(Json(json!({
        "profile": {
            "id": id,
            "name": name,
            "type": scholar_type,
            "joined_at": joined_at,
            "contributions": {
                "skill_runs": skill_runs,
                "reviews": reviews,
            },
            "capabilities": capabilities,
        }
    })))
}

async fn ensure_exists(state: &AppState, table: &str, id: &str, message: &str) -> ApiResult<()> {
    let query = match table {
        "research_agendas" => "SELECT 1 FROM research_agendas WHERE id = $1",
        "hypotheses" => "SELECT 1 FROM hypotheses WHERE id = $1",
        "scholars" => "SELECT 1 FROM scholars WHERE id = $1",
        _ => return Err(ApiError::internal("Unsupported existence check")),
    };

    let exists: Option<(i32,)> = sqlx::query_as(query)
        .bind(id)
        .fetch_optional(&state.db)
        .await?;

    if exists.is_some() {
        Ok(())
    } else {
        Err(ApiError::not_found(message))
    }
}

async fn count_for(state: &AppState, table: &str, column: &str, value: &str) -> ApiResult<i64> {
    let query = match (table, column) {
        ("skill_runs", "agent_id") => "SELECT COUNT(*) FROM skill_runs WHERE agent_id = $1",
        ("reviews", "reviewer_id") => "SELECT COUNT(*) FROM reviews WHERE reviewer_id = $1",
        _ => return Err(ApiError::internal("Unsupported count query")),
    };
    let (count,): (i64,) = sqlx::query_as(query)
        .bind(value)
        .fetch_one(&state.db)
        .await?;
    Ok(count)
}
