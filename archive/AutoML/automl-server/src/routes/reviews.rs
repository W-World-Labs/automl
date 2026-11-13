// ============================================================
// Reviews routes — /api/v1/papers/:id/reviews, /api/v1/personas
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
        .route("/papers/{id}/reviews", get(get_reviews).post(submit_review))
        .route("/papers/{id}/rebuttal", post(submit_rebuttal))
        .route("/personas", get(list_personas))
}

// ── GET /papers/:id/reviews ──────────────────────────────────

async fn get_reviews(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let reviews: Vec<(String, String, String, String, i64, i64, String, String, String, String)> =
        sqlx::query_as(
            "SELECT id, paper_id, reviewer_id, verdict, score, confidence, summary, strengths, weaknesses, created_at \
             FROM reviews WHERE paper_id = $1 ORDER BY created_at ASC"
        )
        .bind(&id)
        .fetch_all(&state.db)
        .await?;

    let items: Vec<Value> = reviews
        .iter()
        .map(|r| {
            json!({
                "id": r.0, "paper_id": r.1, "reviewer_id": r.2,
                "verdict": r.3, "score": r.4, "confidence": r.5,
                "summary": r.6,
                "strengths": serde_json::from_str::<Value>(&r.7).unwrap_or(json!([])),
                "weaknesses": serde_json::from_str::<Value>(&r.8).unwrap_or(json!([])),
                "created_at": r.9,
            })
        })
        .collect();

    Ok(Json(json!({
        "paper_id": id,
        "reviews": items,
        "total": items.len(),
    })))
}

// ── POST /papers/:id/reviews — Submit review ─────────────────

#[derive(Debug, Deserialize)]
struct ReviewSubmit {
    verdict: String,
    score: i64,
    confidence: i64,
    summary: String,
    strengths: Vec<String>,
    weaknesses: Vec<String>,
}

async fn submit_review(
    State(state): State<AppState>,
    user: AuthUser,
    Path(paper_id): Path<String>,
    Json(body): Json<ReviewSubmit>,
) -> ApiResult<Json<Value>> {
    // Validate verdict
    if !["ACCEPT", "BORDERLINE", "REJECT"].contains(&body.verdict.as_str()) {
        return Err(ApiError::bad_request(
            "verdict must be ACCEPT, BORDERLINE, or REJECT",
        ));
    }
    if body.score < 1 || body.score > 10 {
        return Err(ApiError::bad_request("score must be 1-10"));
    }
    if body.confidence < 1 || body.confidence > 5 {
        return Err(ApiError::bad_request("confidence must be 1-5"));
    }

    // Verify paper exists
    let paper: Option<(String, String)> =
        sqlx::query_as("SELECT id, status FROM papers WHERE id = $1")
            .bind(&paper_id)
            .fetch_optional(&state.db)
            .await?;

    let (_, status) = paper.ok_or_else(|| ApiError::not_found("Paper not found"))?;
    if status != "UNDER_REVIEW" && status != "REBUTTAL" {
        return Err(ApiError::bad_request(format!(
            "Paper must be UNDER_REVIEW or REBUTTAL to review (currently: {})",
            status
        )));
    }

    let review_id = format!("rev_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO reviews (id, paper_id, reviewer_id, verdict, score, confidence, summary, strengths, weaknesses, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
    )
    .bind(&review_id)
    .bind(&paper_id)
    .bind(&user.scholar_id)
    .bind(&body.verdict)
    .bind(body.score)
    .bind(body.confidence)
    .bind(&body.summary)
    .bind(serde_json::to_string(&body.strengths).unwrap_or_default())
    .bind(serde_json::to_string(&body.weaknesses).unwrap_or_default())
    .bind(&now)
    .execute(&state.db)
    .await?;

    // Log activity
    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, 'review', $2, $3, $4, $5)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(format!("Review submitted: {} (score: {}/10)", body.verdict, body.score))
    .bind(&user.scholar_id)
    .bind(&paper_id)
    .bind(&now)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "review_id": review_id,
        "paper_id": paper_id,
        "verdict": body.verdict,
        "score": body.score,
    })))
}

// ── POST /papers/:id/rebuttal ────────────────────────────────

#[derive(Debug, Deserialize)]
struct RebuttalSubmit {
    content: String,
    point_by_point: Option<Vec<Value>>,
}

async fn submit_rebuttal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(paper_id): Path<String>,
    Json(body): Json<RebuttalSubmit>,
) -> ApiResult<Json<Value>> {
    let paper: Option<(String, String)> =
        sqlx::query_as("SELECT id, status FROM papers WHERE id = $1")
            .bind(&paper_id)
            .fetch_optional(&state.db)
            .await?;

    let (_, status) = paper.ok_or_else(|| ApiError::not_found("Paper not found"))?;
    if status != "REBUTTAL" {
        return Err(ApiError::bad_request(format!(
            "Paper must be in REBUTTAL stage (currently: {})",
            status
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();

    // Store rebuttal as activity event with full content
    let rebuttal_data = json!({
        "content": body.content,
        "point_by_point": body.point_by_point,
        "submitted_by": user.scholar_id,
    });

    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, 'rebuttal', $2, $3, $4, $5)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(rebuttal_data.to_string())
    .bind(&user.scholar_id)
    .bind(&paper_id)
    .bind(&now)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "paper_id": paper_id,
        "message": "Rebuttal submitted. The paper will be re-reviewed.",
    })))
}

// ── GET /personas ────────────────────────────────────────────

async fn list_personas() -> Json<Value> {
    Json(json!({
        "personas": [
            {
                "id": "reproducer",
                "name": "The Reproducer",
                "role": "Reproducibility verification",
                "focus": "Can the experiments be reproduced? Are dependencies clear?",
            },
            {
                "id": "librarian",
                "name": "The Librarian",
                "role": "Reference verification",
                "focus": "Are citations accurate? Is prior work properly acknowledged?",
            },
            {
                "id": "reviewer2",
                "name": "Reviewer #2",
                "role": "Critical analysis",
                "focus": "Methodology rigor, statistical validity, logical consistency",
            },
            {
                "id": "area-chair",
                "name": "The Area Chair",
                "role": "Meta-review synthesis",
                "focus": "Overall quality assessment, recommendation synthesis",
            },
            {
                "id": "security",
                "name": "Security Auditor",
                "role": "Safety and ethics review",
                "focus": "Dual-use concerns, data privacy, ethical considerations",
            },
        ]
    }))
}
