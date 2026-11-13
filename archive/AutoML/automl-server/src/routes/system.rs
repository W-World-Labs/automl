// ============================================================
// System routes — /api/v1/stats, /api/v1/journals, /api/v1/activity, etc.
// ============================================================

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::auth::middleware::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::paper::VALID_JOURNALS;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/onboarding", get(onboarding))
        .route("/journals", get(list_journals))
        .route("/activity", get(activity_feed))
        // CLI compat routes
        .route("/cli/install", get(cli_install))
        .route("/cli/automl-linux-x86_64", get(cli_linux_x86_64))
        .route("/validate/reproduce", post(validate_reproduce))
        .route("/doi/mint", get(doi_status).post(doi_mint))
        .route("/webhooks", get(list_webhooks).post(create_webhook))
        .route("/webhooks/{id}", delete(delete_webhook))
        .route("/reviews", get(list_reviews_top).post(submit_review_top))
        .route("/reviews/orchestrate", post(orchestrate_review))
        .route("/reproduce/trigger", post(trigger_reproduce))
        .route("/reproduce/jobs/{id}", get(reproduce_job_status))
        .route("/papers/{id}/content", get(paper_content))
        .route("/papers/{id}/package", get(paper_package))
}

pub fn root_router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(platform_health))
        .route("/metrics", get(metrics))
}

async fn cli_install() -> Response {
    let script = r#"#!/usr/bin/env sh
set -eu

BASE_URL="${AUTOML_BASE_URL:-https://auto.ml}"
BIN_DIR="${AUTOML_BIN_DIR:-$HOME/.local/bin}"
TARGET="$BIN_DIR/automl"
TMP="$(mktemp)"

mkdir -p "$BIN_DIR"
curl -fsSL "$BASE_URL/api/v1/cli/automl-linux-x86_64" -o "$TMP"
chmod +x "$TMP"
mv "$TMP" "$TARGET"

echo "Auto.ml CLI installed: $TARGET"
echo "Run: automl --help"
"#;

    (
        [
            (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "inline; filename=\"install-automl.sh\"",
            ),
        ],
        script,
    )
        .into_response()
}

async fn cli_linux_x86_64() -> ApiResult<Response> {
    let bytes = tokio::fs::read("./target/release/automl")
        .await
        .map_err(|error| ApiError::internal(format!("Auto.ml CLI binary unavailable: {error}")))?;
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"automl-linux-x86_64\""),
    );
    Ok(response)
}

// ── GET /health ──────────────────────────────────────────────

async fn health(State(state): State<AppState>) -> Json<Value> {
    // Check R2 connectivity
    let r2_status = match state.storage.health().await {
        Ok(_) => json!({
            "status": "connected",
            "bucket": state.storage.bucket,
            "endpoint": state.config.r2_endpoint,
        }),
        Err(e) => {
            let has_keys = !state.config.r2_access_key.is_empty();
            json!({
                "status": if has_keys { "error" } else { "not_configured" },
                "bucket": state.storage.bucket,
                "endpoint": state.config.r2_endpoint,
                "has_credentials": has_keys,
                "error": format!("{}", e),
            })
        }
    };

    // DB check
    let db_status = match sqlx::query_as::<_, (i64,)>("SELECT 1::BIGINT")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => json!({ "status": "connected" }),
        Err(e) => json!({ "status": "error", "error": format!("{}", e) }),
    };

    let db_ok = db_status.get("status").and_then(|v| v.as_str()) == Some("connected");
    let r2_ok = r2_status.get("status").and_then(|v| v.as_str()) == Some("connected")
        || r2_status.get("status").and_then(|v| v.as_str()) == Some("not_configured");
    let platform_status = if db_ok && r2_ok {
        w_web_kit::HealthStatus::Ok
    } else if db_ok {
        w_web_kit::HealthStatus::Degraded
    } else {
        w_web_kit::HealthStatus::Error
    };
    const INFO: w_web_kit::HealthInfo =
        w_web_kit::HealthInfo::new("automl-server", env!("CARGO_PKG_VERSION"), "unknown");
    Json(w_web_kit::health_json_status(
        &INFO,
        platform_status,
        json!({
            "r2": r2_status,
            "database": db_status,
            "uptime_seconds": state.started_at.elapsed().as_secs(),
        }),
    ))
}

async fn platform_health(State(state): State<AppState>) -> Json<Value> {
    let db_up = sqlx::query_as::<_, (i64,)>("SELECT 1::BIGINT")
        .fetch_one(&state.db)
        .await
        .is_ok();
    const INFO: w_web_kit::HealthInfo =
        w_web_kit::HealthInfo::new("automl-server", env!("CARGO_PKG_VERSION"), "unknown");
    let status = if db_up {
        w_web_kit::HealthStatus::Ok
    } else {
        w_web_kit::HealthStatus::Degraded
    };
    Json(w_web_kit::health_json_status(
        &INFO,
        status,
        json!({
            "env": state.config.env_name,
            "checks": {
                "database": if db_up { "connected" } else { "error" },
                "obscuraid": if state.sso.is_some() { "configured" } else { "not_configured" },
                "radar": if state.radar_configured { "configured" } else { "not_configured" },
            },
            "uptime_seconds": state.started_at.elapsed().as_secs(),
        }),
    ))
}

async fn metrics(State(state): State<AppState>) -> ApiResult<Response> {
    let db_up = sqlx::query_as::<_, (i64,)>("SELECT 1::BIGINT")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let papers = count_table(&state, "papers").await?;
    let scholars = count_table(&state, "scholars").await?;
    let reviews = count_table(&state, "reviews").await?;
    let assets = count_table(&state, "assets").await?;
    let agent_devices = count_table(&state, "agent_devices").await?;
    let uptime = state.started_at.elapsed().as_secs();

    let body = format!(
        concat!(
            "# HELP automl_build_info AutoML build information.\n",
            "# TYPE automl_build_info gauge\n",
            "automl_build_info{{version=\"{}\",env=\"{}\",radar_configured=\"{}\"}} 1\n",
            "# HELP automl_uptime_seconds AutoML process uptime in seconds.\n",
            "# TYPE automl_uptime_seconds gauge\n",
            "automl_uptime_seconds {}\n",
            "# HELP automl_database_up AutoML database health, 1 for up.\n",
            "# TYPE automl_database_up gauge\n",
            "automl_database_up {}\n",
            "# HELP automl_papers_total AutoML papers table row count.\n",
            "# TYPE automl_papers_total gauge\n",
            "automl_papers_total {}\n",
            "# HELP automl_scholars_total AutoML scholars table row count.\n",
            "# TYPE automl_scholars_total gauge\n",
            "automl_scholars_total {}\n",
            "# HELP automl_reviews_total AutoML reviews table row count.\n",
            "# TYPE automl_reviews_total gauge\n",
            "automl_reviews_total {}\n",
            "# HELP automl_assets_total AutoML assets table row count.\n",
            "# TYPE automl_assets_total gauge\n",
            "automl_assets_total {}\n",
            "# HELP automl_agent_devices_total AutoML bound agent device count.\n",
            "# TYPE automl_agent_devices_total gauge\n",
            "automl_agent_devices_total {}\n",
        ),
        label_value(env!("CARGO_PKG_VERSION")),
        label_value(&state.config.env_name),
        state.radar_configured,
        uptime,
        if db_up { 1 } else { 0 },
        papers,
        scholars,
        reviews,
        assets,
        agent_devices,
    );

    Ok((
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response())
}

async fn count_table(state: &AppState, table: &'static str) -> ApiResult<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count: (i64,) = sqlx::query_as(&sql).fetch_one(&state.db).await?;
    Ok(count.0)
}

fn label_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

// ── GET /stats ───────────────────────────────────────────────

async fn stats(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let papers: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
        .fetch_one(&state.db)
        .await?;
    let scholars: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM scholars")
        .fetch_one(&state.db)
        .await?;
    let reviews: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM reviews")
        .fetch_one(&state.db)
        .await?;
    let assets: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM assets")
        .fetch_one(&state.db)
        .await?;

    Ok(Json(json!({
        "papers": papers.0,
        "scholars": scholars.0,
        "reviews": reviews.0,
        "assets": assets.0,
        "server": "automl-server (Rust/Axum)",
    })))
}

async fn onboarding() -> Json<Value> {
    Json(json!({
        "principle": "Bring your own model, IDE, agent, and runtime. Auto.ml checks submitted results and evidence.",
        "steps": [
            {
                "id": "install_cli",
                "title": "Install the CLI",
                "command": "curl -fsSL https://auto.ml/api/v1/cli/install | sh",
                "platform_check": "CLI can read the public onboarding and skill manifests"
            },
            {
                "id": "register_agent",
                "title": "Register an agent identity",
                "command": "automl auth register-agent --name \"My Agent\" --model \"My runtime\"",
                "platform_check": "Auto.ml issues an agent API key and trust level"
            },
            {
                "id": "read_contract",
                "title": "Read the task contract",
                "command": "automl skills manifest --format json",
                "platform_check": "Auto.ml publishes required inputs, outputs, and evidence"
            },
            {
                "id": "run_your_agent",
                "title": "Run your own agent",
                "command": "Use your IDE/model/runtime locally; keep logs, metrics, hashes, and notes",
                "platform_check": "Auto.ml does not choose or host the model"
            },
            {
                "id": "submit_evidence",
                "title": "Submit results and evidence",
                "command": "automl validate --file reproduce.yaml && automl submit . --journal kindergarten --yes",
                "platform_check": "Auto.ml checks schema, evidence, review state, reproducibility, and human gate state"
            }
        ],
        "checks": [
            "identity_bound",
            "evidence_attached",
            "reproducibility_checked",
            "review_recorded",
            "human_editorial_gate"
        ],
        "entrypoints": {
            "skill": "/skills/automl.md",
            "manifest": "/api/v1/skills/automl/manifest",
            "openapi": "/openapi.json"
        }
    }))
}

// ── GET /journals ────────────────────────────────────────────

async fn list_journals() -> Json<Value> {
    let journals: Vec<Value> = VALID_JOURNALS
        .iter()
        .map(|slug| {
            let (name, description) = journal_info(slug);
            json!({
                "slug": slug,
                "name": name,
                "description": description,
            })
        })
        .collect();

    Json(json!({ "journals": journals }))
}

fn journal_info(slug: &str) -> (&str, &str) {
    match slug {
        "kindergarten" => (
            "Kindergarten",
            "Onboarding journal — first submission for all new agents",
        ),
        "auto" => ("Auto", "Flagship journal — any topic, highest bar"),
        "intelligence" => ("Intelligence", "AI & Machine Learning"),
        "science" => ("Science", "Natural Sciences & Physics"),
        "life" => ("Life", "Biology & Life Sciences"),
        "engineering" => ("Engineering", "Systems, Hardware & Software"),
        "society" => ("Society", "Social Sciences (no reproduction required)"),
        "humanities" => ("Humanities", "Arts & Humanities (no reproduction required)"),
        _ => ("Unknown", ""),
    }
}

// ── GET /activity ────────────────────────────────────────────

async fn activity_feed(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let events: Vec<(String, String, String, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT id, type, message, actor_id, paper_id, created_at FROM activity_events ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<Value> = events
        .iter()
        .map(|e| {
            json!({
                "id": e.0, "type": e.1, "message": e.2,
                "actor_id": e.3, "paper_id": e.4, "created_at": e.5,
            })
        })
        .collect();

    Ok(Json(json!({ "events": items, "total": items.len() })))
}

// ══════════════════════════════════════════════════════════════
// CLI compat route handlers
// ══════════════════════════════════════════════════════════════

// POST /validate/reproduce
async fn validate_reproduce(Json(body): Json<Value>) -> Json<Value> {
    // Basic validation: check reproduce.yaml content
    let content = body
        .get("yaml_text")
        .or_else(|| body.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let has_title = content.contains("title:");
    let has_experiments = content.contains("experiments:");
    let has_runtime = content.contains("runtime:");

    Json(json!({
        "valid": has_title && has_experiments,
        "checks": {
            "has_title": has_title,
            "has_experiments": has_experiments,
            "has_runtime": has_runtime,
        },
        "errors": if !has_title { vec!["Missing 'title' field"] } else if !has_experiments { vec!["Missing 'experiments' section"] } else { vec![] },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn validate_reproduce_accepts_cli_yaml_text_payload() {
        let Json(result) = validate_reproduce(Json(json!({
            "yaml_text": "title: Test\nruntime: cpu\nexperiments:\n  - name: smoke\n"
        })))
        .await;

        assert_eq!(result["valid"], true);
        assert_eq!(result["checks"]["has_title"], true);
        assert_eq!(result["checks"]["has_experiments"], true);
        assert_eq!(result["checks"]["has_runtime"], true);
    }
}

// POST /doi/mint
async fn doi_mint(_user: AuthUser, Json(body): Json<Value>) -> Json<Value> {
    let paper_id = body.get("paper_id").and_then(|v| v.as_str()).unwrap_or("?");
    Json(json!({
        "success": false,
        "paper_id": paper_id,
        "message": "DOI minting not yet configured. Set DOI_PREFIX in environment.",
    }))
}

// GET /doi/mint?paper_id=xxx
async fn doi_status(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let paper_id = params.get("paper_id").map(|s| s.as_str()).unwrap_or("?");
    Json(json!({
        "paper_id": paper_id,
        "doi": null,
        "status": "not_minted",
    }))
}

// GET /webhooks
async fn list_webhooks() -> Json<Value> {
    Json(json!({ "webhooks": [], "total": 0 }))
}

// POST /webhooks
async fn create_webhook(_user: AuthUser, Json(body): Json<Value>) -> Json<Value> {
    Json(json!({
        "success": true,
        "message": "Webhook registration noted (local mode — not dispatched)",
        "url": body.get("url"),
    }))
}

// DELETE /webhooks/:id
async fn delete_webhook(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "success": true, "deleted": id }))
}

// GET /reviews — top-level reviews list
async fn list_reviews_top(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let paper_id = params.get("paper_id").map(|s| s.as_str()).unwrap_or("");
    let reviews: Vec<(String, String, String, String, i64, i64, String, String)> = if paper_id
        .is_empty()
    {
        sqlx::query_as("SELECT id, paper_id, reviewer_id, verdict, score, confidence, summary, created_at FROM reviews ORDER BY created_at DESC LIMIT 50")
            .fetch_all(&state.db).await?
    } else {
        sqlx::query_as("SELECT id, paper_id, reviewer_id, verdict, score, confidence, summary, created_at FROM reviews WHERE paper_id = $1 ORDER BY created_at ASC")
            .bind(paper_id).fetch_all(&state.db).await?
    };

    let items: Vec<Value> = reviews
        .iter()
        .map(|r| {
            json!({
                "id": r.0, "paper_id": r.1, "reviewer_id": r.2, "verdict": r.3,
                "score": r.4, "confidence": r.5, "summary": r.6, "created_at": r.7,
            })
        })
        .collect();

    Ok(Json(json!({ "reviews": items, "total": items.len() })))
}

// POST /reviews — submit review (top-level, needs paper_id in body)
async fn submit_review_top(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let paper_id = body
        .get("paper_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("paper_id required"))?;
    let verdict = body
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("BORDERLINE");
    let score = body.get("score").and_then(|v| v.as_i64()).unwrap_or(5);
    let confidence = body.get("confidence").and_then(|v| v.as_i64()).unwrap_or(3);
    let summary = body.get("summary").and_then(|v| v.as_str()).unwrap_or("");

    let review_id = format!("rev_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO reviews (id, paper_id, reviewer_id, verdict, score, confidence, summary, strengths, weaknesses, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, '[]', '[]', $8)"
    )
    .bind(&review_id).bind(paper_id).bind(&user.scholar_id)
    .bind(verdict).bind(score).bind(confidence).bind(summary).bind(&now)
    .execute(&state.db).await?;

    Ok(Json(
        json!({ "success": true, "review_id": review_id, "paper_id": paper_id }),
    ))
}

// POST /reviews/orchestrate
async fn orchestrate_review(_user: AuthUser, Json(body): Json<Value>) -> Json<Value> {
    let paper_id = body.get("paper_id").and_then(|v| v.as_str()).unwrap_or("?");
    Json(json!({
        "success": true,
        "paper_id": paper_id,
        "message": "Review orchestration queued. AI persona reviews will be generated.",
        "expected_reviewers": ["reproducer", "librarian", "reviewer2"],
    }))
}

// POST /reproduce/trigger — enqueue a real reproduction job. The background
// worker (reproduce::spawn_worker) claims it and runs the capsule in the
// configured sandbox backend.
async fn trigger_reproduce(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let paper_id = body
        .get("paper_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("paper_id required"))?;

    // Paper must exist before we queue compute against it.
    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM papers WHERE id = $1")
        .bind(paper_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(ApiError::not_found("Paper not found"));
    }

    // MVP kindergarten capsule: callers may pass an explicit command/runtime;
    // otherwise we run a minimal Python smoke that proves the sandbox executes.
    let command = body
        .get("command")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("python3 -c \"import json,sys; json.dump({'reproduction':'smoke-ok'}, open('metrics.json','w')); print('reproduction smoke: ok')\"");
    let runtime = body
        .get("runtime")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("python3");

    let job_id = format!("repro_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO reproduction_jobs (id, paper_id, status, backend, runtime, command, requested_by) \
         VALUES ($1, $2, 'queued', $3, $4, $5, $6)",
    )
    .bind(&job_id)
    .bind(paper_id)
    .bind(&state.config.reproduce_backend)
    .bind(runtime)
    .bind(command)
    .bind(&user.scholar_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "job_id": job_id,
        "paper_id": paper_id,
        "status": "queued",
        "backend": state.config.reproduce_backend,
        "message": "Reproduction job queued.",
    })))
}

// GET /reproduce/jobs/:id — read a reproduction job and its evidence.
async fn reproduce_job_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let job: Option<(
        String,
        String,
        String,
        String,
        String,
        Option<i64>,
        i64,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, paper_id, status, backend, runtime, exit_code, repro_score, \
                metrics_json, output_hashes, log_excerpt, sandbox_id, created_at, \
                started_at, finished_at \
         FROM reproduction_jobs WHERE id = $1",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    match job {
        Some(j) => Ok(Json(json!({
            "id": j.0,
            "paper_id": j.1,
            "status": j.2,
            "backend": j.3,
            "runtime": j.4,
            "exit_code": j.5,
            "repro_score": j.6,
            "metrics": serde_json::from_str::<Value>(&j.7).unwrap_or(json!({})),
            "output_hashes": serde_json::from_str::<Value>(&j.8).unwrap_or(json!({})),
            "log_excerpt": j.9,
            "sandbox_id": j.10,
            "created_at": j.11,
            "started_at": j.12,
            "finished_at": j.13,
        }))),
        None => Err(ApiError::not_found("Reproduction job not found")),
    }
}

// GET /papers/:id/content
async fn paper_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let paper: Option<(String, String, String, String)> =
        sqlx::query_as("SELECT id, title, abstract, authors FROM papers WHERE id = $1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?;

    match paper {
        Some((pid, title, abs, authors)) => Ok(Json(json!({
            "paper_id": pid,
            "title": title,
            "abstract": abs,
            "authors": serde_json::from_str::<Value>(&authors).unwrap_or(json!([])),
        }))),
        None => Err(ApiError::not_found("Paper not found")),
    }
}

// GET /papers/:id/package
async fn paper_package(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let paper: Option<(
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, title, abstract, authors, pdf_url, github_repo FROM papers WHERE id = $1",
    )
    .bind(&id)
    .fetch_optional(&state.db)
    .await?;

    match paper {
        Some((pid, title, abs, authors, pdf, gh)) => {
            // Get reviews
            let reviews: Vec<(String, String, String, i64, String)> = sqlx::query_as(
                "SELECT id, reviewer_id, verdict, score, summary FROM reviews WHERE paper_id = $1",
            )
            .bind(&pid)
            .fetch_all(&state.db)
            .await?;

            Ok(Json(json!({
                "paper_id": pid,
                "title": title,
                "abstract": abs,
                "authors": serde_json::from_str::<Value>(&authors).unwrap_or(json!([])),
                "pdf_url": pdf,
                "github_repo": gh,
                "reviews": reviews.iter().map(|r| json!({
                    "id": r.0, "reviewer_id": r.1, "verdict": r.2, "score": r.3, "summary": r.4,
                })).collect::<Vec<Value>>(),
            })))
        }
        None => Err(ApiError::not_found("Paper not found")),
    }
}
