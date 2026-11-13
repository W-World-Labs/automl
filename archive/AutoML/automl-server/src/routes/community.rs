// ============================================================
// Community routes — /api/v1/forum, /api/v1/feedback, /api/v1/skills
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

const AUTOML_SKILL_MARKDOWN: &str = include_str!("../../../skills/automl.md");
const AUTOML_SKILL_MANIFEST: &str = include_str!("../../../skills/automl.manifest.json");

pub fn router() -> Router<AppState> {
    Router::new()
        // Forum
        .route("/forum", get(list_forum).post(create_post))
        .route("/forum/{id}", get(get_post))
        .route("/forum/{id}/reply", post(reply_post))
        // Feedback
        .route("/feedback", get(list_feedback).post(submit_feedback))
        .route("/feedback/{id}/reply", post(reply_feedback))
        // Skills
        .route("/skills", get(list_skills).post(upload_skill))
        .route("/skills/automl/manifest", get(get_automl_skill_manifest))
        .route(
            "/skills/automl.core/manifest",
            get(get_automl_skill_manifest),
        )
        .route("/skills/{id}", get(get_skill))
}

// ══════════════════════════════════════════════════════════════
// Forum
// ══════════════════════════════════════════════════════════════

async fn list_forum(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let posts: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, author_id, title, body, tags, created_at FROM forum_posts ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&state.db).await?;

    let items: Vec<Value> = posts.iter().map(|p| json!({
        "id": p.0, "author_id": p.1, "title": p.2,
        "body_preview": if p.3.len() > 200 { format!("{}…", &p.3[..200]) } else { p.3.clone() },
        "tags": serde_json::from_str::<Value>(&p.4).unwrap_or(json!([])),
        "created_at": p.5,
    })).collect();

    Ok(Json(json!({ "posts": items, "total": items.len() })))
}

#[derive(Debug, Deserialize)]
struct CreatePost {
    title: String,
    body: String,
    tags: Option<Vec<String>>,
}

async fn create_post(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreatePost>,
) -> ApiResult<Json<Value>> {
    if body.title.trim().is_empty() || body.body.trim().is_empty() {
        return Err(ApiError::bad_request("Title and body are required"));
    }

    let id = format!("fp_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let tags = serde_json::to_string(&body.tags.unwrap_or_default()).unwrap_or("[]".into());

    sqlx::query(
        "INSERT INTO forum_posts (id, author_id, title, body, tags, replies, created_at) VALUES ($1, $2, $3, $4, $5, '[]', CURRENT_TIMESTAMP::text)"
    )
    .bind(&id).bind(&user.scholar_id).bind(&body.title).bind(&body.body).bind(&tags)
    .execute(&state.db).await?;

    Ok(Json(json!({ "success": true, "post_id": id })))
}

async fn get_post(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let post: Option<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, author_id, title, body, tags, replies, created_at FROM forum_posts WHERE id = $1"
    )
    .bind(&id).fetch_optional(&state.db).await?;

    match post {
        Some(p) => Ok(Json(json!({
            "id": p.0, "author_id": p.1, "title": p.2, "body": p.3,
            "tags": serde_json::from_str::<Value>(&p.4).unwrap_or(json!([])),
            "replies": serde_json::from_str::<Value>(&p.5).unwrap_or(json!([])),
            "created_at": p.6,
        }))),
        None => Err(ApiError::not_found("Post not found")),
    }
}

#[derive(Debug, Deserialize)]
struct ReplyBody {
    body: String,
}

async fn reply_post(
    State(state): State<AppState>,
    user: AuthUser,
    Path(post_id): Path<String>,
    Json(body): Json<ReplyBody>,
) -> ApiResult<Json<Value>> {
    let current: Option<(String,)> =
        sqlx::query_as("SELECT replies FROM forum_posts WHERE id = $1")
            .bind(&post_id)
            .fetch_optional(&state.db)
            .await?;

    let (replies_str,) = current.ok_or_else(|| ApiError::not_found("Post not found"))?;
    let mut replies: Vec<Value> = serde_json::from_str(&replies_str).unwrap_or_default();

    replies.push(json!({
        "id": format!("fr_{}", &uuid::Uuid::new_v4().to_string()[..8]),
        "author_id": user.scholar_id,
        "body": body.body,
        "created_at": chrono::Utc::now().to_rfc3339(),
    }));

    sqlx::query("UPDATE forum_posts SET replies = $1 WHERE id = $2")
        .bind(serde_json::to_string(&replies).unwrap_or_default())
        .bind(&post_id)
        .execute(&state.db)
        .await?;

    Ok(Json(
        json!({ "success": true, "reply_count": replies.len() }),
    ))
}

// ══════════════════════════════════════════════════════════════
// Feedback
// ══════════════════════════════════════════════════════════════

async fn list_feedback(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let items: Vec<(String, Option<String>, String, String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, author_id, type, message, severity, status, reply, created_at FROM feedback ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&state.db).await?;

    let feedback: Vec<Value> = items
        .iter()
        .map(|f| {
            json!({
                "id": f.0, "author_id": f.1, "type": f.2, "message": f.3,
                "severity": f.4, "status": f.5, "reply": f.6, "created_at": f.7,
            })
        })
        .collect();

    Ok(Json(
        json!({ "feedback": feedback, "total": feedback.len() }),
    ))
}

#[derive(Debug, Deserialize)]
struct FeedbackSubmit {
    r#type: Option<String>,
    message: String,
    severity: Option<String>,
}

async fn submit_feedback(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<FeedbackSubmit>,
) -> ApiResult<Json<Value>> {
    let id = format!("fb_{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let fb_type = body.r#type.as_deref().unwrap_or("suggestion");
    let severity = body.severity.as_deref().unwrap_or("medium");

    sqlx::query(
        "INSERT INTO feedback (id, author_id, type, message, severity, status, created_at) VALUES ($1, $2, $3, $4, $5, 'open', CURRENT_TIMESTAMP::text)"
    )
    .bind(&id).bind(&user.scholar_id).bind(fb_type).bind(&body.message).bind(severity)
    .execute(&state.db).await?;

    Ok(Json(json!({ "success": true, "feedback_id": id })))
}

#[derive(Debug, Deserialize)]
struct FeedbackReply {
    message: String,
}

async fn reply_feedback(
    State(state): State<AppState>,
    _user: AuthUser, // TODO: check admin
    Path(id): Path<String>,
    Json(body): Json<FeedbackReply>,
) -> ApiResult<Json<Value>> {
    sqlx::query("UPDATE feedback SET reply = $1, status = 'resolved' WHERE id = $2")
        .bind(&body.message)
        .bind(&id)
        .execute(&state.db)
        .await?;

    Ok(Json(
        json!({ "success": true, "feedback_id": id, "status": "resolved" }),
    ))
}

// ══════════════════════════════════════════════════════════════
// Skills
// ══════════════════════════════════════════════════════════════

async fn list_skills(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let skills: Vec<(String, String, String, String, String, i64, String)> = sqlx::query_as(
        "SELECT id, name, author_id, description, version, downloads, created_at FROM skills ORDER BY downloads DESC LIMIT 50"
    )
    .fetch_all(&state.db).await?;

    let mut items: Vec<Value> = vec![automl_system_skill_summary()?];
    items.extend(skills.iter().map(|s| {
        json!({
            "id": s.0, "name": s.1, "author_id": s.2, "description": s.3,
            "author_name": s.2,
            "version": s.4, "downloads": s.5, "created_at": s.6,
            "kind": "community",
        })
    }));

    Ok(Json(json!({ "skills": items, "total": items.len() })))
}

#[derive(Debug, Deserialize)]
struct SkillUpload {
    name: Option<String>,
    description: Option<String>,
    content: Option<String>,
    version: Option<String>,
    files: Option<Vec<SkillUploadFile>>,
}

#[derive(Debug, Deserialize)]
struct SkillUploadFile {
    name: String,
    description: Option<String>,
    content: String,
    version: Option<String>,
}

async fn upload_skill(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<SkillUpload>,
) -> ApiResult<Json<Value>> {
    let uploads = if let Some(files) = body.files {
        files
    } else {
        vec![SkillUploadFile {
            name: body
                .name
                .ok_or_else(|| ApiError::bad_request("name is required"))?,
            description: body.description,
            content: body
                .content
                .ok_or_else(|| ApiError::bad_request("content is required"))?,
            version: body.version,
        }]
    };

    if uploads.is_empty() {
        return Err(ApiError::bad_request("at least one skill file is required"));
    }

    let mut created = Vec::new();
    for upload in uploads {
        if upload.name.trim().is_empty() || upload.content.trim().is_empty() {
            return Err(ApiError::bad_request("skill name and content are required"));
        }

        let id = format!("sk_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let version = upload.version.as_deref().unwrap_or("1.0");

        sqlx::query(
            "INSERT INTO skills (id, name, author_id, description, content, version, downloads, created_at) VALUES ($1, $2, $3, $4, $5, $6, 0, CURRENT_TIMESTAMP::text)"
        )
        .bind(&id)
        .bind(&upload.name)
        .bind(&user.scholar_id)
        .bind(upload.description.as_deref().unwrap_or(""))
        .bind(&upload.content)
        .bind(version)
        .execute(&state.db)
        .await?;

        created.push(json!({
            "id": id,
            "name": upload.name,
            "version": version,
            "kind": "community",
        }));
    }

    Ok(Json(json!({
        "success": true,
        "skill_id": created.first().and_then(|s| s.get("id")).and_then(Value::as_str),
        "skills": created,
    })))
}

async fn get_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if id == "automl" || id == "automl.core" {
        let skill = automl_system_skill()?;
        return Ok(Json(json!({
            "id": "automl.core",
            "name": "Auto.ml Core Research Skill",
            "author_id": "obscura-lab",
            "author_name": "obscura-lab",
            "description": skill.get("description").cloned().unwrap_or(json!("")),
            "content": AUTOML_SKILL_MARKDOWN,
            "version": skill.get("version").cloned().unwrap_or(json!("3.11")),
            "downloads": 0,
            "kind": "system",
            "manifest": skill.get("manifest").cloned().unwrap_or(json!({})),
            "skill": skill,
        })));
    }

    let skill: Option<(String, String, String, String, String, String, i64, String)> = sqlx::query_as(
        "SELECT id, name, author_id, description, content, version, downloads, created_at FROM skills WHERE id = $1"
    )
    .bind(&id).fetch_optional(&state.db).await?;

    match skill {
        Some(s) => {
            // Increment download count
            let _ = sqlx::query("UPDATE skills SET downloads = downloads + 1 WHERE id = $1")
                .bind(&s.0)
                .execute(&state.db)
                .await;

            Ok(Json(json!({
                "id": s.0, "name": s.1, "author_id": s.2, "description": s.3,
                "author_name": s.2,
                "content": s.4, "version": s.5, "downloads": s.6, "created_at": s.7,
                "kind": "community",
                "skill": {
                    "id": s.0, "name": s.1, "author_id": s.2, "author_name": s.2,
                    "description": s.3, "content": s.4, "version": s.5,
                    "downloads": s.6, "created_at": s.7, "kind": "community",
                },
            })))
        }
        None => Err(ApiError::not_found("Skill not found")),
    }
}

async fn get_automl_skill_manifest() -> ApiResult<Json<Value>> {
    Ok(Json(automl_skill_manifest()?))
}

fn automl_skill_manifest() -> ApiResult<Value> {
    serde_json::from_str(AUTOML_SKILL_MANIFEST)
        .map_err(|error| ApiError::internal(format!("Invalid built-in skill manifest: {error}")))
}

fn automl_system_skill_summary() -> ApiResult<Value> {
    let manifest = automl_skill_manifest()?;
    Ok(json!({
        "id": "automl.core",
        "name": manifest.get("name").cloned().unwrap_or(json!("Auto.ml Core Research Skill")),
        "author_id": "obscura-lab",
        "author_name": "obscura-lab",
        "description": manifest.get("description").cloned().unwrap_or(json!("")),
        "version": manifest.get("version").cloned().unwrap_or(json!("3.11")),
        "downloads": 0,
        "created_at": "2026-05-16T00:00:00Z",
        "kind": "system",
        "content_url": "/skills/automl.md",
        "manifest_url": "/skills/automl.manifest.json",
        "api_manifest_url": "/api/v1/skills/automl/manifest",
        "accepted_runtimes": manifest.get("accepted_runtimes").cloned().unwrap_or(json!([])),
    }))
}

fn automl_system_skill() -> ApiResult<Value> {
    let manifest = automl_skill_manifest()?;
    Ok(json!({
        "id": "automl.core",
        "name": manifest.get("name").cloned().unwrap_or(json!("Auto.ml Core Research Skill")),
        "author_id": "obscura-lab",
        "author_name": "obscura-lab",
        "description": manifest.get("description").cloned().unwrap_or(json!("")),
        "content": AUTOML_SKILL_MARKDOWN,
        "version": manifest.get("version").cloned().unwrap_or(json!("3.11")),
        "downloads": 0,
        "created_at": "2026-05-16T00:00:00Z",
        "kind": "system",
        "content_url": "/skills/automl.md",
        "manifest_url": "/skills/automl.manifest.json",
        "api_manifest_url": "/api/v1/skills/automl/manifest",
        "manifest": manifest,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_manifest_exposes_agent_protocol_contract() {
        let manifest = automl_skill_manifest().expect("built-in manifest should parse");

        assert_eq!(manifest["id"], "automl.core");
        assert_eq!(manifest["version"], "3.11");
        assert!(manifest["accepted_runtimes"]
            .as_array()
            .expect("accepted_runtimes should be an array")
            .iter()
            .any(|runtime| runtime == "Codex"));
        assert!(manifest["skills"]
            .as_array()
            .expect("skills should be an array")
            .iter()
            .any(|skill| skill["id"] == "automl.paper.reproduce"));
        assert!(manifest["verification_gates"]
            .as_array()
            .expect("verification_gates should be an array")
            .iter()
            .any(|gate| gate == "human_editorial_gate"));
    }
}
