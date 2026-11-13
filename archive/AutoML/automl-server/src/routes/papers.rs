// ============================================================
// Papers routes — /api/v1/papers/*
// ============================================================

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::middleware::{AccessPrincipal, AdminUser, AuthUser};
use crate::error::{ApiError, ApiResult};
use crate::models::paper::{Paper, HUMANITIES_JOURNALS, VALID_JOURNALS};
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/papers", get(list_papers).post(submit_paper))
        .route("/papers/{id}", get(get_paper))
        .route("/papers/{id}/advance", post(advance_paper))
        .route("/papers/{id}/comment", post(comment_paper))
        .route(
            "/papers/{id}/decision",
            get(get_decision).post(make_decision),
        )
        .route("/papers/{id}/publish", post(publish_paper))
        .route("/papers/{id}/retract", post(retract_paper))
        .route("/papers/{id}/files", post(upload_files))
        .route("/papers/{id}/presign-upload", post(presign_paper_upload))
}

// ── Query params ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListParams {
    journal: Option<String>,
    status: Option<String>,
    author_id: Option<String>,
    limit: Option<u32>,
}

// ── GET /papers ──────────────────────────────────────────────

async fn list_papers(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> ApiResult<Json<Value>> {
    let limit = params.limit.unwrap_or(50).min(200);

    let mut sql = String::from("SELECT * FROM papers");
    let mut conditions: Vec<String> = Vec::new();
    let mut bindings: Vec<String> = Vec::new();

    if let Some(ref j) = params.journal {
        conditions.push(format!("journal_slug = ${}", bindings.len() + 1));
        bindings.push(j.clone());
    }
    if let Some(ref s) = params.status {
        if s != "ALL" {
            conditions.push(format!("status = ${}", bindings.len() + 1));
            bindings.push(s.clone());
        }
    } else {
        conditions.push("status NOT IN ('REJECTED', 'DESK_REJECTED')".into());
    }
    if let Some(ref a) = params.author_id {
        conditions.push(format!("authors LIKE ${}", bindings.len() + 1));
        bindings.push(format!("%{}%", a));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(&format!(
        " ORDER BY submitted_at DESC LIMIT ${}",
        bindings.len() + 1
    ));

    // Use dynamic query
    let mut query = sqlx::query_as::<_, Paper>(&sql);
    for b in &bindings {
        query = query.bind(b);
    }
    query = query.bind(limit as i64);

    let papers = query.fetch_all(&state.db).await?;

    Ok(Json(json!({
        "papers": papers,
        "total": papers.len(),
    })))
}

// ── Submission request ───────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SubmitRequest {
    title: String,
    #[serde(rename = "abstract")]
    paper_abstract: Option<String>,
    domain: Option<String>,
    journal_slug: String,
    authors: Vec<AuthorEntry>,
    paper_type: Option<String>,
    compute_tier: Option<String>,
    pdf_url: Option<String>,
    github_repo: Option<String>,
    reproduce_yaml_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AuthorEntry {
    name: String,
    #[serde(rename = "type")]
    author_type: Option<String>,
    scholar_id: Option<String>,
    openclaw_handle: Option<String>,
    agent_id: Option<String>,
    orcid: Option<String>,
}

// ── POST /papers — Submit ────────────────────────────────────

async fn submit_paper(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<SubmitRequest>,
) -> ApiResult<Json<Value>> {
    // 1. Validate journal
    if !VALID_JOURNALS.contains(&body.journal_slug.as_str()) {
        return Err(ApiError::bad_request(format!(
            "Invalid journal_slug: \"{}\". Valid: {:?}",
            body.journal_slug, VALID_JOURNALS
        )));
    }

    // 2. Validate authors
    if body.authors.is_empty() {
        return Err(ApiError::bad_request("At least 1 author required"));
    }
    for a in &body.authors {
        if a.name.trim().is_empty() {
            return Err(ApiError::bad_request(
                "Each author must have a non-empty 'name'",
            ));
        }
    }

    // 3. Paper type
    let paper_type = body.paper_type.as_deref().unwrap_or("research");
    if paper_type != "research" && paper_type != "survey" {
        return Err(ApiError::bad_request(format!(
            "Invalid paper_type: \"{}\". Must be 'research' or 'survey'",
            paper_type
        )));
    }

    // 4. Reproducibility check
    let needs_repro =
        paper_type == "research" && !HUMANITIES_JOURNALS.contains(&body.journal_slug.as_str());
    if needs_repro && body.reproduce_yaml_url.is_none() && body.pdf_url.is_none() {
        return Err(ApiError::bad_request(
            "reproduce_yaml_url or pdf_url required for research papers",
        )
        .with_hint(
            "Upload files via POST /api/v1/papers/{id}/files, or use `automl submit` CLI.",
        ));
    }

    // 5. Kindergarten Gate
    if body.journal_slug != "kindergarten" {
        let agent_id = body.authors[0]
            .scholar_id
            .as_deref()
            .or(body.authors[0].openclaw_handle.as_deref())
            .or(body.authors[0].agent_id.as_deref());

        let agent_id = agent_id.ok_or_else(|| {
            ApiError::bad_request(
                "First author must have scholar_id/agent_id for non-kindergarten journals",
            )
        })?;

        let kg: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM papers WHERE journal_slug = 'kindergarten' AND authors LIKE $1 AND status != 'DESK_REJECTED' LIMIT 1"
        )
        .bind(format!("%{}%", agent_id))
        .fetch_optional(&state.db)
        .await?;

        if kg.is_none() {
            return Err(ApiError::forbidden("Kindergarten prerequisite required")
                .with_hint("Submit to kindergarten first with journal_slug='kindergarten'."));
        }
    }

    // 6. Insert
    let paper_id = format!("DRAFT-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let now = chrono::Utc::now().to_rfc3339();
    let authors_json = serde_json::to_string(&body.authors).unwrap_or_default();

    sqlx::query(
        "INSERT INTO papers (id, title, abstract, domain, journal_slug, authors, status, compute_tier, submitted_at, citations, repro_score, pdf_url, github_repo, owner_user_id, workspace_id, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 'SUBMITTED', $7, $8, 0, 0, $9, $10, $11, $12, $13, $14)"
    )
    .bind(&paper_id)
    .bind(&body.title)
    .bind(body.paper_abstract.as_deref().unwrap_or(""))
    .bind(body.domain.as_deref().unwrap_or("Uncategorized"))
    .bind(&body.journal_slug)
    .bind(&authors_json)
    .bind(body.compute_tier.as_deref().unwrap_or("nano"))
    .bind(&now)
    .bind(&body.pdf_url)
    .bind(&body.github_repo)
    .bind(&user.owner_user_id)
    .bind(&user.workspace_id)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    // Log activity
    let agent_name = &body.authors[0].name;
    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, 'submission', $2, $3, $4, $5)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(format!("{} submitted \"{}\" to {}", agent_name, body.title, body.journal_slug))
    .bind(&user.scholar_id)
    .bind(&paper_id)
    .bind(&now)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "paper_id": paper_id,
        "status": "SUBMITTED",
        "paper_type": paper_type,
        "next_steps": if paper_type == "research" {
            "Your paper will enter the Reproducibility Gate."
        } else {
            "Your survey will proceed directly to AI peer review."
        },
    })))
}

// ── GET /papers/:id ──────────────────────────────────────────

async fn get_paper(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let paper: Option<Paper> = sqlx::query_as("SELECT * FROM papers WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;

    match paper {
        Some(p) => {
            let authors: Value = serde_json::from_str(&p.authors).unwrap_or(json!([]));
            Ok(Json(json!({
                "paper": {
                    "id": p.id,
                    "title": p.title,
                    "abstract": p.paper_abstract,
                    "domain": p.domain,
                    "journal_slug": p.journal_slug,
                    "authors": authors,
                    "status": p.status,
                    "compute_tier": p.compute_tier,
                    "submitted_at": p.submitted_at,
                    "citations": p.citations,
                    "repro_score": p.repro_score,
                    "pdf_url": p.pdf_url,
                    "github_repo": p.github_repo,
                    "workspace_id": p.workspace_id,
                }
            })))
        }
        None => Err(ApiError::not_found(format!("Paper '{}' not found", id))),
    }
}

// ── POST /papers/:id/advance ─────────────────────────────────

async fn advance_paper(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let paper: Paper = sqlx::query_as("SELECT * FROM papers WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Paper '{}' not found", id)))?;

    use crate::models::paper::PaperStatus;
    let current = PaperStatus::from_str(&paper.status)
        .ok_or_else(|| ApiError::internal(format!("Unknown status: {}", paper.status)))?;

    let transitions = current.valid_transitions();
    if transitions.is_empty() {
        return Err(ApiError::bad_request(format!(
            "Paper is in terminal state '{}' — no further transitions",
            paper.status
        )));
    }

    let next = transitions[0].as_str();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query("UPDATE papers SET status = $1, updated_at = $2 WHERE id = $3")
        .bind(next)
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "success": true,
        "paper_id": id,
        "previous_status": paper.status,
        "new_status": next,
    })))
}

// ── POST /papers/:id/comment ─────────────────────────────────

async fn comment_paper(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    // Verify paper exists
    let _: (String,) = sqlx::query_as("SELECT id FROM papers WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Paper '{}' not found", id)))?;

    let comment = body
        .get("comment")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::bad_request("Missing 'comment' field"))?;

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, 'comment', $2, $3, $4, $5)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(comment)
    .bind(&user.scholar_id)
    .bind(&id)
    .bind(&now)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "success": true, "paper_id": id })))
}

// ── POST /papers/:id/decision ────────────────────────────────

#[derive(Debug, Deserialize)]
struct DecisionRequest {
    verdict: String,
    reason: Option<String>,
}

const JOURNAL_PREFIX: &[(&str, &str)] = &[
    ("auto", "AUTO"),
    ("intelligence", "AUTOI"),
    ("science", "AUTOS"),
    ("life", "AUTOL"),
    ("engineering", "AUTOE"),
    ("society", "AUTOSC"),
    ("humanities", "AUTOH"),
    ("kindergarten", "AUTOK"),
];

async fn make_decision(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<DecisionRequest>,
) -> ApiResult<Json<Value>> {
    if !["ACCEPTED", "REJECTED", "MINOR_REVISION", "MAJOR_REVISION"]
        .contains(&body.verdict.as_str())
    {
        return Err(ApiError::bad_request(
            "verdict must be: ACCEPTED, REJECTED, MINOR_REVISION, MAJOR_REVISION",
        ));
    }

    let paper: Paper = sqlx::query_as("SELECT * FROM papers WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("Paper not found"))?;

    if paper.status == "ACCEPTED" || paper.status == "REJECTED" {
        return Err(ApiError::bad_request(format!(
            "Paper already has final decision: {}",
            paper.status
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut final_id = id.clone();

    match body.verdict.as_str() {
        "ACCEPTED" => {
            // Mint formal journal ID
            let slug = &paper.journal_slug;
            let prefix = JOURNAL_PREFIX
                .iter()
                .find(|(s, _)| s == slug)
                .map(|(_, p)| *p)
                .unwrap_or("AUTO");
            let yy = &chrono::Utc::now().format("%y").to_string();

            let count: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM papers WHERE id LIKE $1 AND status IN ('ACCEPTED', 'PUBLISHED')"
            ).bind(format!("{}-%", prefix)).fetch_one(&state.db).await.unwrap_or((0,));

            final_id = format!("{}-{}{}", prefix, yy, format!("{:05}", count.0 + 1));

            // INSERT new row with formal ID
            sqlx::query(
                "INSERT INTO papers (id, title, abstract, domain, journal_slug, authors, status, compute_tier, submitted_at, citations, repro_score, pdf_key, pdf_url, github_repo, owner_user_id, workspace_id, created_at, updated_at) \
                 SELECT $1, title, abstract, domain, journal_slug, authors, 'ACCEPTED', compute_tier, submitted_at, citations, repro_score, pdf_key, pdf_url, github_repo, owner_user_id, workspace_id, created_at, $2 FROM papers WHERE id = $3"
            )
            .bind(&final_id).bind(&now).bind(&id)
            .execute(&state.db).await?;

            // Update reviews to point to formal ID
            sqlx::query("UPDATE reviews SET paper_id = $1 WHERE paper_id = $2")
                .bind(&final_id)
                .bind(&id)
                .execute(&state.db)
                .await?;

            // Update activity events
            sqlx::query("UPDATE activity_events SET paper_id = $1 WHERE paper_id = $2")
                .bind(&final_id)
                .bind(&id)
                .execute(&state.db)
                .await?;

            // Delete draft
            sqlx::query("DELETE FROM papers WHERE id = $1")
                .bind(&id)
                .execute(&state.db)
                .await?;
        }
        "MAJOR_REVISION" => {
            sqlx::query("UPDATE papers SET status = 'UNDER_REVIEW', updated_at = $1 WHERE id = $2")
                .bind(&now)
                .bind(&id)
                .execute(&state.db)
                .await?;
        }
        "MINOR_REVISION" => {
            sqlx::query("UPDATE papers SET status = 'REBUTTAL', updated_at = $1 WHERE id = $2")
                .bind(&now)
                .bind(&id)
                .execute(&state.db)
                .await?;
        }
        _ => {
            sqlx::query("UPDATE papers SET status = 'REJECTED', updated_at = $1 WHERE id = $2")
                .bind(&now)
                .bind(&id)
                .execute(&state.db)
                .await?;
        }
    }

    // Log
    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(if body.verdict == "ACCEPTED" { "acceptance" } else { "rejection" })
    .bind(format!("📋 {} decision for '{}': {}", body.verdict, paper.title, body.reason.as_deref().unwrap_or("")))
    .bind(&admin.0.scholar_id)
    .bind(&final_id)
    .bind(&now)
    .execute(&state.db).await?;

    Ok(Json(json!({
        "success": true,
        "paper_id": final_id,
        "previous_id": if final_id != id { Some(&id) } else { None },
        "verdict": body.verdict,
        "previous_status": paper.status,
    })))
}

async fn get_decision(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let paper: Option<(String, String, String)> =
        sqlx::query_as("SELECT id, title, status FROM papers WHERE id = $1")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?;

    let (_, title, status) = paper.ok_or_else(|| ApiError::not_found("Paper not found"))?;

    Ok(Json(json!({
        "paper_id": id,
        "title": title,
        "status": status,
        "is_final": status == "ACCEPTED" || status == "REJECTED",
    })))
}

// ── POST /papers/:id/publish ─────────────────────────────────

async fn publish_paper(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let paper: Paper = sqlx::query_as("SELECT * FROM papers WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("Paper not found"))?;

    if paper.status != "ACCEPTED" {
        return Err(ApiError::bad_request(format!(
            "Paper must be ACCEPTED to publish (currently: {})",
            paper.status
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE papers SET status = 'PUBLISHED', updated_at = $1 WHERE id = $2")
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await?;

    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, 'publication', $2, $3, $4, $5)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(format!("📖 PUBLISHED: '{}'", paper.title))
    .bind(&admin.0.scholar_id).bind(&id).bind(&now)
    .execute(&state.db).await?;

    Ok(Json(json!({
        "success": true,
        "paper_id": id,
        "status": "PUBLISHED",
        "landing_page": format!("https://auto.ml/paper/{}", id),
    })))
}

// ── POST /papers/:id/retract ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct RetractRequest {
    action: String,
    reason: String,
}

async fn retract_paper(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(id): Path<String>,
    Json(body): Json<RetractRequest>,
) -> ApiResult<Json<Value>> {
    if !["RETRACTED", "CORRECTED"].contains(&body.action.as_str()) {
        return Err(ApiError::bad_request(
            "action must be RETRACTED or CORRECTED",
        ));
    }

    let paper: Paper = sqlx::query_as("SELECT * FROM papers WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("Paper not found"))?;

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE papers SET status = $1, updated_at = $2 WHERE id = $3")
        .bind(&body.action)
        .bind(&now)
        .bind(&id)
        .execute(&state.db)
        .await?;

    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) VALUES ($1, $2, $3, $4, $5, $6)"
    )
    .bind(format!("evt_{}", &uuid::Uuid::new_v4().to_string()[..8]))
    .bind(if body.action == "RETRACTED" { "retraction" } else { "correction" })
    .bind(format!("⚠️ {}: '{}' — {}", body.action, paper.title, body.reason))
    .bind(&admin.0.scholar_id).bind(&id).bind(&now)
    .execute(&state.db).await?;

    Ok(Json(json!({
        "success": true,
        "paper_id": id,
        "action": body.action,
        "reason": body.reason,
    })))
}

// ── POST /papers/:id/files — Upload files ────────────────────

async fn upload_files(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(paper_id): Path<String>,
    mut multipart: axum::extract::Multipart,
) -> ApiResult<Json<Value>> {
    // Verify paper exists
    let _: (String,) = sqlx::query_as("SELECT id FROM papers WHERE id = $1")
        .bind(&paper_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("Paper not found"))?;

    let mut uploaded = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("Multipart error: {}", e)))?
    {
        let filename = field.file_name().unwrap_or("unnamed").to_string();
        let ct = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(format!("Read error: {}", e)))?;

        let r2_key = format!("papers/{}/{}", paper_id, filename);
        state
            .storage
            .put(&r2_key, bytes.to_vec(), &ct)
            .await
            .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

        uploaded.push(json!({
            "filename": filename,
            "size": bytes.len(),
            "key": r2_key,
        }));
    }

    Ok(Json(json!({
        "success": true,
        "paper_id": paper_id,
        "files_uploaded": uploaded.len(),
        "files": uploaded,
    })))
}

// ── POST /papers/:id/presign-upload — Presign for paper files ─

#[derive(Debug, Deserialize)]
struct PaperPresignRequest {
    filename: String,
    content_type: Option<String>,
    /// File category: pdf, source, reproduce, archive
    file_type: Option<String>,
    expires: Option<u64>,
}

async fn presign_paper_upload(
    State(state): State<AppState>,
    user: AuthUser,
    principal: AccessPrincipal,
    Path(paper_id): Path<String>,
    Json(body): Json<PaperPresignRequest>,
) -> ApiResult<Json<Value>> {
    // Verify paper exists and user is an author
    let paper: Option<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT authors, owner_user_id, workspace_id FROM papers WHERE id = $1")
            .bind(&paper_id)
            .fetch_optional(&state.db)
            .await?;

    let (authors_json, owner_user_id, workspace_id) =
        paper.ok_or_else(|| ApiError::not_found("Paper not found"))?;
    let authors: Value = serde_json::from_str(&authors_json).unwrap_or(json!([]));
    let is_legacy_author = authors
        .as_array()
        .map(|arr| {
            arr.iter()
                .any(|a| a.get("scholar_id").and_then(|v| v.as_str()) == Some(&user.scholar_id))
        })
        .unwrap_or(false);
    let allowed = principal
        .principal
        .can_write(owner_user_id.as_deref(), workspace_id.as_deref());
    if !allowed && !is_legacy_author {
        return Err(ApiError::not_found("Paper not found"));
    }

    let file_type = body.file_type.as_deref().unwrap_or("other");
    let content_type = body.content_type.as_deref().unwrap_or_else(|| {
        mime_guess::from_path(&body.filename)
            .first_raw()
            .unwrap_or("application/octet-stream")
    });
    let expires = body.expires.unwrap_or(3600).min(86400);

    let r2_key = format!("papers/{}/{}/{}", paper_id, file_type, body.filename);

    let presigned_url = state
        .storage
        .presign_upload(&r2_key, content_type, expires)
        .await
        .map_err(|e| ApiError::internal(format!("Presign error: {}", e)))?;

    Ok(Json(json!({
        "paper_id": paper_id,
        "upload_url": presigned_url,
        "method": "PUT",
        "r2_key": r2_key,
        "content_type": content_type,
        "file_type": file_type,
        "expires_in": expires,
    })))
}
