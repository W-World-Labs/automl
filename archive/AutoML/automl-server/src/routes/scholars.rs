// ============================================================
// Scholars routes — /api/v1/scholars/*, /api/v1/agents/*, /api/v1/auth/*
// ============================================================

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::api_key;
use crate::auth::middleware::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::scholar::Scholar;
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scholars", get(list_scholars).post(register_scholar))
        .route("/scholars/{id}", get(get_scholar))
        .route("/scholars/resolve", get(list_scholars)) // alias
        .route("/agents", post(register_agent_authenticated))
        .route("/agents/anonymous", post(register_agent_anonymous)) // CLI compat alias
        .route("/agents/bind-device", post(bind_device))
        .route("/auth/keys", post(generate_key))
}

// ── GET /scholars ────────────────────────────────────────────

async fn list_scholars(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let scholars: Vec<Scholar> =
        sqlx::query_as("SELECT * FROM scholars ORDER BY joined_at DESC LIMIT 100")
            .fetch_all(&state.db)
            .await?;

    Ok(Json(json!({
        "scholars": scholars,
        "total": scholars.len(),
    })))
}

// ── GET /scholars/:id ────────────────────────────────────────

async fn get_scholar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let scholar: Option<Scholar> = sqlx::query_as("SELECT * FROM scholars WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;

    match scholar {
        Some(s) => Ok(Json(json!({ "scholar": s }))),
        None => Err(ApiError::not_found(format!("Scholar '{}' not found", id))),
    }
}

// ── POST /scholars — Register human scholar ──────────────────

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    name: String,
    affiliation: Option<String>,
    orcid: Option<String>,
    bio: Option<String>,
}

async fn register_scholar(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<RegisterRequest>,
) -> ApiResult<Json<Value>> {
    if body.name.trim().is_empty() {
        return Err(ApiError::bad_request("Name is required"));
    }

    let id = format!("sci_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO scholars (id, name, type, affiliation, bio, badges, orcid, joined_at, created_at, updated_at) \
         VALUES ($1, $2, 'human', $3, $4, '[]', $5, $6, $7, $8)"
    )
    .bind(&id)
    .bind(&body.name)
    .bind(body.affiliation.as_deref().unwrap_or(""))
    .bind(body.bio.as_deref().unwrap_or(""))
    .bind(&body.orcid)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    // Auto-generate API key
    let api_key = api_key::create_api_key(&state.db, &id, "Default")
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create API key: {}", e)))?;

    Ok(Json(json!({
        "success": true,
        "scholar_id": id,
        "api_key": api_key,
        "name": body.name,
        "type": "human",
        "registered_by": user.owner_user_id,
        "usage": {
            "header": format!("Authorization: Bearer {}", api_key),
            "cli": format!("automl auth set-key {}", api_key),
        }
    })))
}

// ── POST /agents — Register AI agent ────────────────────────

#[derive(Debug, Deserialize)]
struct AgentRegisterRequest {
    name: Option<String>,
    handle: Option<String>,
    model: Option<String>,
}

async fn register_agent_authenticated(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<AgentRegisterRequest>,
) -> ApiResult<Json<Value>> {
    create_agent(state, Some(user), body).await
}

async fn register_agent_anonymous(
    State(state): State<AppState>,
    Json(body): Json<AgentRegisterRequest>,
) -> ApiResult<Json<Value>> {
    create_agent(state, None, body).await
}

async fn create_agent(
    state: AppState,
    user: Option<AuthUser>,
    body: AgentRegisterRequest,
) -> ApiResult<Json<Value>> {
    let agent_name = body.name.as_deref().unwrap_or("Anonymous Agent");
    let handle = body.handle.clone();
    let model = body.model.clone();
    let id = format!("agent_{}", uuid::Uuid::new_v4().simple());
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO scholars (id, name, type, affiliation, bio, badges, openclaw_handle, lineage, joined_at, created_at, updated_at) \
         VALUES ($1, $2, 'agent', '', '', '[]', $3, $4, $5, $6, $7)"
    )
    .bind(&id)
    .bind(agent_name)
    .bind(&handle)
    .bind(&model)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    // Create API key
    let api_key = api_key::create_api_key(&state.db, &id, "Agent Key")
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create API key: {}", e)))?;

    // Create enrollment token for device binding. Store only the token hash.
    let enroll_token = format!(
        "enroll_{}",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    );
    let enroll_hash = api_key::hash_token(&enroll_token);
    sqlx::query(
        "INSERT INTO agent_enrollments (id, agent_id, token_hash, created_at, expires_at) \
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP::text, (CURRENT_TIMESTAMP + INTERVAL '24 hours')::text)",
    )
    .bind(format!("enr_{}", uuid::Uuid::new_v4().simple()))
    .bind(&id)
    .bind(&enroll_hash)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "agent_id": &id,
        "scholar_id": &id,
        "api_key": api_key,
        "enrollment_token": enroll_token,
        "device_id": Value::Null,
        "trust_level": "unverified",
        "name": agent_name,
        "openclaw_handle": handle,
        "type": "agent",
        "registered_by": user.as_ref().map(|u| u.owner_user_id.as_str()).unwrap_or(&id),
        "workspace_id": user.and_then(|u| u.workspace_id),
        "commands": {
            "bind_device": format!("automl auth bind-device {}", enroll_token),
            "submit": "automl submit . --journal kindergarten",
        },
        "next_steps": {
            "bind_device": format!("automl auth bind-device {}", enroll_token),
            "submit": "automl submit . --journal kindergarten",
        }
    })))
}

// ── POST /auth/keys — Generate new API key (requires auth) ──

#[derive(Debug, Deserialize)]
struct KeyRequest {
    name: Option<String>,
}

async fn generate_key(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<KeyRequest>,
) -> ApiResult<Json<Value>> {
    let key_name = body.name.as_deref().unwrap_or("CLI Key");

    let new_key = api_key::create_api_key(&state.db, &user.scholar_id, key_name)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create API key: {}", e)))?;

    Ok(Json(json!({
        "success": true,
        "api_key": new_key,
        "key_name": key_name,
        "scholar_id": user.scholar_id,
        "usage": {
            "header": format!("Authorization: Bearer {}", new_key),
            "cli": format!("automl auth set-key {}", new_key),
        }
    })))
}

// ── POST /agents/bind-device ─────────────────────────────────

#[derive(Debug, Deserialize)]
struct BindDeviceRequest {
    enrollment_token: Option<String>,
    device_fingerprint: Option<String>,
    device: Option<Value>,
    hostname: Option<String>,
    gpu_info: Option<Value>,
}

async fn bind_device(
    State(state): State<AppState>,
    Json(body): Json<BindDeviceRequest>,
) -> ApiResult<Json<Value>> {
    let token = body
        .enrollment_token
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("enrollment_token is required"))?;
    let token_hash = api_key::hash_token(token);
    let enrollment: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, agent_id, used_at FROM agent_enrollments \
         WHERE token_hash = $1 AND expires_at::timestamp > CURRENT_TIMESTAMP",
    )
    .bind(&token_hash)
    .fetch_optional(&state.db)
    .await?;

    let (enrollment_id, agent_id, used_at) =
        enrollment.ok_or_else(|| ApiError::unauthorized("Invalid or expired enrollment token"))?;
    if used_at.is_some() {
        return Err(ApiError::unauthorized(
            "Enrollment token has already been used",
        ));
    }

    let device_id = format!("dev_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    let device_fingerprint = body
        .device_fingerprint
        .clone()
        .or_else(|| body.device.as_ref().map(|device| device.to_string()))
        .unwrap_or_else(|| "{}".to_string());
    let hostname = body.hostname.clone().or_else(|| {
        body.device
            .as_ref()
            .and_then(|device| device.get("hostname_hash"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
    });
    let gpu_info = body
        .gpu_info
        .as_ref()
        .map(Value::to_string)
        .or_else(|| body.device.as_ref().map(Value::to_string));

    sqlx::query(
        "INSERT INTO agent_devices \
         (id, agent_id, hostname, gpu_info, enrollment_token, device_fingerprint, enrolled_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP::text, CURRENT_TIMESTAMP::text)",
    )
    .bind(&device_id)
    .bind(&agent_id)
    .bind(&hostname)
    .bind(&gpu_info)
    .bind(&token_hash)
    .bind(&device_fingerprint)
    .execute(&state.db)
    .await?;

    sqlx::query("UPDATE agent_enrollments SET used_at = CURRENT_TIMESTAMP::text WHERE id = $1")
        .bind(&enrollment_id)
        .execute(&state.db)
        .await?;

    let api_key = api_key::create_api_key(&state.db, &agent_id, "Bound Device Key")
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create API key: {}", e)))?;

    Ok(Json(json!({
        "success": true,
        "message": "Device binding accepted",
        "device_id": device_id,
        "agent_id": agent_id,
        "scholar_id": agent_id,
        "api_key": api_key,
        "trust_level": "unverified",
    })))
}
