// ============================================================
// Compute routes — /api/v1/compute/*
// ============================================================

use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::error::ApiResult;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/compute", get(overview))
        .route("/compute/nodes", get(list_nodes))
        .route("/compute/jobs", get(list_jobs))
}

async fn overview(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    // Count papers by compute tier
    let tiers: Vec<(String, i64)> =
        sqlx::query_as("SELECT compute_tier, COUNT(*) FROM papers GROUP BY compute_tier")
            .fetch_all(&state.db)
            .await?;

    let tier_counts: Value = tiers
        .iter()
        .map(|(t, c)| (t.clone(), json!(c)))
        .collect::<serde_json::Map<String, Value>>()
        .into();

    let active_jobs: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM reproduction_jobs WHERE status IN ('queued', 'running')",
    )
    .fetch_one(&state.db)
    .await?;
    let queue_depth: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM reproduction_jobs WHERE status = 'queued'")
            .fetch_one(&state.db)
            .await?;

    Ok(Json(json!({
        "swarm": {
            "status": "operational",
            "backend": state.config.reproduce_backend,
            "total_nodes": 1,
            "active_jobs": active_jobs.0,
            "queue_depth": queue_depth.0,
        },
        "tier_distribution": tier_counts,
    })))
}

async fn list_nodes(State(state): State<AppState>) -> Json<Value> {
    // The reproduction worker is an in-process node backed by the configured
    // sandbox backend (local subprocess or CubeSandbox on the configured sandbox host).
    Json(json!({
        "nodes": [{
            "id": "reproduction-worker",
            "backend": state.config.reproduce_backend,
            "status": if state.config.reproduce_execute { "online" } else { "execution_disabled" },
            "executes": state.config.reproduce_execute,
        }],
        "total": 1,
    }))
}

async fn list_jobs(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let jobs: Vec<(
        String,
        String,
        String,
        String,
        i64,
        Option<i64>,
        String,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, paper_id, status, backend, repro_score, exit_code, created_at, finished_at \
             FROM reproduction_jobs ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<Value> = jobs
        .iter()
        .map(|j| {
            json!({
                "id": j.0, "paper_id": j.1, "status": j.2, "backend": j.3,
                "repro_score": j.4, "exit_code": j.5, "created_at": j.6, "finished_at": j.7,
            })
        })
        .collect();

    Ok(Json(json!({ "jobs": items, "total": items.len() })))
}
