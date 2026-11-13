// ============================================================
// Auth middleware — extract caller identity from request
// ============================================================

use crate::auth::{api_key, session};
use crate::routes::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use w_id_sdk::tenancy::{Identity, Principal};

/// Authenticated caller identity. API keys and W-ID sessions both map
/// onto the local scholar id so legacy CLI handlers keep working.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub scholar_id: String,
    pub key_name: String,
    pub trust_level: String,
    pub owner_user_id: String,
    pub workspace_id: Option<String>,
}

/// Principal used for user-owned resource checks.
#[derive(Debug, Clone)]
pub struct AccessPrincipal {
    pub principal: Principal,
}

/// Admin user — requires trust_level = 'admin' or key in ADMIN_KEYS
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

// ── Required auth extractor ──────────────────────────────────
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(token) = bearer_token(parts) {
            if let Some(user) = api_key_user(state, token).await {
                return Ok(user);
            }
            return Err(unauthorized(
                "Invalid or expired API key",
                "Generate a new key via POST /api/v1/auth/keys",
            ));
        }

        match sso_session_user(parts, state).await {
            Ok(Some(user)) => Ok(user),
            Ok(None) => Err(unauthorized(
                "Authentication required",
                "Include 'Authorization: Bearer aml_xxx' or login with W-ID",
            )),
            Err(error) => {
                tracing::error!("Session auth lookup failed: {}", error);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Authentication lookup failed" })),
                )
                    .into_response())
            }
        }
    }
}

// ── Principal extractor for ACL checks ───────────────────────
impl FromRequestParts<AppState> for AccessPrincipal {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        let is_admin = matches!(user.trust_level.as_str(), "admin" | "platform");
        let identity = Identity {
            sub: user.owner_user_id.clone(),
            login: user.key_name.clone(),
            is_admin,
            workspaces: user.workspace_id.iter().cloned().collect(),
            demo_workspaces: Vec::new(),
        };
        Ok(Self {
            principal: Principal::User(identity),
        })
    }
}

// ── Admin auth extractor (requires admin trust_level) ────────
impl FromRequestParts<AppState> for AdminUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;

        // Fail-closed: elevating every authenticated user to admin is only
        // allowed when both conditions hold — non-production AND explicit
        // ALLOW_DEV_ADMIN=1. Empty ADMIN_KEYS alone must never imply admin
        // (issue #1 / historical open-elevation backdoor).
        if state.config.dev_admin_elevation_enabled() {
            return Ok(AdminUser(user));
        }

        if user.trust_level == "admin" || user.trust_level == "platform" {
            Ok(AdminUser(user))
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Admin access required",
                    "hint": "This operation requires admin privileges. Contact platform admin.",
                    "your_trust_level": user.trust_level,
                })),
            )
                .into_response())
        }
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
}

async fn api_key_user(state: &AppState, token: &str) -> Option<AuthUser> {
    let info = api_key::verify_api_key(&state.db, token).await?;

    // Check if this key is in ADMIN_KEYS — auto-promote to admin trust level
    let token_hash = api_key::hash_token(token);
    let trust = if state
        .config
        .admin_keys
        .iter()
        .any(|key| key == token || key == &token_hash)
    {
        "admin".to_string()
    } else {
        info.trust_level
    };

    Some(AuthUser {
        owner_user_id: info.scholar_id.clone(),
        workspace_id: None,
        scholar_id: info.scholar_id,
        key_name: info.key_name,
        trust_level: trust,
    })
}

async fn sso_session_user(
    parts: &Parts,
    state: &AppState,
) -> Result<Option<AuthUser>, sqlx::Error> {
    let Some(sso) = state.sso.as_ref() else {
        return Ok(None);
    };
    let Some(token) = session::cookie_value(&parts.headers, session::SESSION_COOKIE) else {
        return Ok(None);
    };
    let Some(decoded) = sso.verify_session(&token) else {
        return Ok(None);
    };

    let scholar: Option<(String,)> =
        sqlx::query_as("SELECT scholar_id FROM sso_users WHERE user_id = $1")
            .bind(&decoded.sub)
            .fetch_optional(&state.db)
            .await?;
    let Some((scholar_id,)) = scholar else {
        return Ok(None);
    };

    let workspaces: Vec<(String,)> =
        sqlx::query_as("SELECT workspace_id FROM workspace_members WHERE user_id = $1")
            .bind(&decoded.sub)
            .fetch_all(&state.db)
            .await?;
    let workspace_id = workspaces.first().map(|row| row.0.clone());

    Ok(Some(AuthUser {
        scholar_id,
        key_name: "W-ID session".to_string(),
        trust_level: if decoded.is_admin {
            "admin".to_string()
        } else {
            "verified".to_string()
        },
        owner_user_id: decoded.sub,
        workspace_id,
    }))
}

fn unauthorized(message: &str, hint: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": message,
            "hint": hint,
        })),
    )
        .into_response()
}
