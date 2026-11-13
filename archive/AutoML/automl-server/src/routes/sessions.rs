// ============================================================
// W-ID session routes — /auth/*
// ============================================================

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use w_id_sdk::tenancy::{Identity, Membership, Principal};

use crate::auth::session::{
    clear_cookie, cookie_value, set_cookie, NEXT_COOKIE, SESSION_COOKIE, STATE_COOKIE,
};
use crate::error::{ApiError, ApiResult};
use crate::routes::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/me", get(me))
        .route("/auth/logout", get(logout).post(logout))
}

#[derive(Debug, Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> ApiResult<Response> {
    let sso = state
        .sso
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("W-ID SSO is not configured"))?;
    let (url, csrf_state) = sso
        .authorize_url()
        .map_err(|e| ApiError::internal(format!("Failed to build authorize URL: {e}")))?;

    let mut response = Redirect::temporary(&url).into_response();
    append_cookie(
        &mut response,
        set_cookie(
            STATE_COOKIE,
            &csrf_state,
            10 * 60,
            true,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    if let Some(next) = sanitized_next(query.next.as_deref()) {
        append_cookie(
            &mut response,
            set_cookie(
                NEXT_COOKIE,
                &next,
                10 * 60,
                true,
                state.config.secure_cookies(),
                state.config.cookie_domain(),
            ),
        )?;
    }
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> ApiResult<Response> {
    if let Some(error) = query.error {
        return Err(ApiError::unauthorized(format!(
            "W-ID rejected login: {error}"
        )));
    }

    let sso = state
        .sso
        .as_ref()
        .ok_or_else(|| ApiError::service_unavailable("W-ID SSO is not configured"))?;
    let code = query
        .code
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("Missing OAuth code"))?;
    let returned_state = query
        .state
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("Missing OAuth state"))?;
    let expected_state = cookie_value(&headers, STATE_COOKIE)
        .ok_or_else(|| ApiError::bad_request("Missing OAuth state cookie"))?;
    if expected_state != returned_state {
        return Err(ApiError::bad_request("OAuth state mismatch"));
    }

    let user = sso
        .exchange_code(code)
        .await
        .map_err(|e| ApiError::unauthorized(format!("W-ID code exchange failed: {e}")))?;
    let session = sso
        .issue_session(&user)
        .map_err(|e| ApiError::internal(format!("Failed to issue local session: {e}")))?;
    let scholar_id = sync_sso_user(&state, &user).await?;

    tracing::info!(
        user_id = %user.sub,
        scholar_id = %scholar_id,
        workspaces = user.workspaces.len(),
        "W-ID login completed"
    );

    let next = cookie_value(&headers, NEXT_COOKIE).unwrap_or_else(|| "/".to_string());
    let next = sanitized_next(Some(&next)).unwrap_or_else(|| "/".to_string());
    let mut response = Redirect::to(&next).into_response();
    append_cookie(
        &mut response,
        set_cookie(
            SESSION_COOKIE,
            &session,
            sso.session_days() * 24 * 60 * 60,
            true,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    append_cookie(
        &mut response,
        clear_cookie(
            STATE_COOKIE,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    append_cookie(
        &mut response,
        clear_cookie(
            NEXT_COOKIE,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    Ok(response)
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let Some(sso) = state.sso.as_ref() else {
        return Ok(anonymous_me_response(
            StatusCode::SERVICE_UNAVAILABLE,
            false,
        ));
    };
    let Some(token) = cookie_value(&headers, SESSION_COOKIE) else {
        return Ok(anonymous_me_response(StatusCode::UNAUTHORIZED, true));
    };
    let Some(session) = sso.verify_session(&token) else {
        return Ok(anonymous_me_response(StatusCode::UNAUTHORIZED, true));
    };

    let workspaces = workspace_ids(&state, &session.sub).await?;
    let scholar_id: Option<(String,)> =
        sqlx::query_as("SELECT scholar_id FROM sso_users WHERE user_id = $1")
            .bind(&session.sub)
            .fetch_optional(&state.db)
            .await?;
    let identity = Identity::from_session(&session, workspaces.clone());
    let principal = Principal::User(identity);

    Ok(Json(json!({
        "authenticated": true,
        "sso_configured": true,
        "user": {
            "sub": session.sub,
            "login": session.login,
            "is_admin": session.is_admin,
            "scholar_id": scholar_id.map(|row| row.0),
        },
        "workspaces": workspaces,
        "principal": {
            "kind": "user",
            "sub": principal.sub(),
            "is_admin": principal.is_admin(),
        }
    }))
    .into_response())
}

fn anonymous_me_response(status: StatusCode, sso_configured: bool) -> Response {
    (
        status,
        Json(json!({
            "authenticated": false,
            "sso_configured": sso_configured,
        })),
    )
        .into_response()
}

async fn logout(State(state): State<AppState>) -> ApiResult<Response> {
    let mut response = Json(json!({ "success": true })).into_response();
    append_cookie(
        &mut response,
        clear_cookie(
            SESSION_COOKIE,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    append_cookie(
        &mut response,
        clear_cookie(
            STATE_COOKIE,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    append_cookie(
        &mut response,
        clear_cookie(
            NEXT_COOKIE,
            state.config.secure_cookies(),
            state.config.cookie_domain(),
        ),
    )?;
    Ok(response)
}

pub async fn workspace_ids(state: &AppState, user_id: &str) -> ApiResult<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT workspace_id FROM workspace_members WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(&state.db)
            .await?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}

async fn sync_sso_user(state: &AppState, user: &w_id_sdk::SsoUser) -> ApiResult<String> {
    let scholar_id = stable_scholar_id(&user.sub);
    let now = chrono::Utc::now().to_rfc3339();
    let fallback_username = user.username();
    let display_name = user
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(fallback_username.as_str())
        .to_string();

    sqlx::query(
        "INSERT INTO scholars (id, name, type, affiliation, bio, badges, joined_at, created_at, updated_at) \
         VALUES ($1, $2, 'human', 'W-ID', 'W-ID authenticated AutoML user', '[\"obscuraid\"]', $3, $4, $5) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at",
    )
    .bind(&scholar_id)
    .bind(&display_name)
    .bind(&now)
    .bind(&now)
    .bind(&now)
    .execute(&state.db)
    .await?;

    sqlx::query(
        "INSERT INTO sso_users (user_id, scholar_id, login, email, name, is_admin, last_login_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT(user_id) DO UPDATE SET \
            scholar_id = excluded.scholar_id, \
            login = excluded.login, \
            email = excluded.email, \
            name = excluded.name, \
            is_admin = excluded.is_admin, \
            last_login_at = excluded.last_login_at",
    )
    .bind(&user.sub)
    .bind(&scholar_id)
    .bind(user.username())
    .bind(&user.email)
    .bind(&user.name)
    .bind(if user.is_platform_admin { 1_i64 } else { 0_i64 })
    .bind(&now)
    .execute(&state.db)
    .await?;

    sqlx::query("DELETE FROM workspace_members WHERE user_id = $1")
        .bind(&user.sub)
        .execute(&state.db)
        .await?;

    for membership in Membership::from_user(user) {
        sqlx::query(
            "INSERT INTO workspace_members \
             (user_id, workspace_id, workspace_name, workspace_type, role, tier, synced_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT(user_id, workspace_id) DO UPDATE SET \
                workspace_name = excluded.workspace_name, \
                workspace_type = excluded.workspace_type, \
                role = excluded.role, \
                tier = excluded.tier, \
                synced_at = excluded.synced_at",
        )
        .bind(&membership.user_id)
        .bind(&membership.workspace_id)
        .bind(&membership.workspace_name)
        .bind(&membership.workspace_type)
        .bind(&membership.role)
        .bind(&membership.tier)
        .bind(&now)
        .execute(&state.db)
        .await?;
    }

    Ok(scholar_id)
}

fn stable_scholar_id(user_id: &str) -> String {
    // KEEP-LOCAL: truncated-hash use (`&hash[..12]`), not a bare full-hex digest;
    // w_crypto::hash::sha256_hex covers full-hex sites only.
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(user_id.as_bytes()));
    format!("sci_sso_{}", &hash[..12])
}

fn sanitized_next(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.len() > 2048 {
        return None;
    }
    if value.starts_with('/') && !value.starts_with("//") {
        return Some(value.to_string());
    }
    if value.starts_with("https://auto.ml/") || value.starts_with("https://scholar.auto.ml/") {
        return Some(value.to_string());
    }
    None
}

fn append_cookie(response: &mut Response, cookie: String) -> ApiResult<()> {
    let value = HeaderValue::from_str(&cookie)
        .map_err(|e| ApiError::internal(format!("Invalid Set-Cookie header: {e}")))?;
    response.headers_mut().append(header::SET_COOKIE, value);
    Ok(())
}

#[cfg(test)]
mod golden {
    //! `sanitized_next` open-redirect guard goldens. Pins automl's exact accept/
    //! reject set, which is NOT byte-equal to w-id-axum `validate_next`: automl
    //! allowlists the `auto.ml` apex hosts, caps length at 2048, and (unlike
    //! validate_next) does NOT reject the `/\` backslash trick or control chars
    //! — so the shared `NextPolicy` cannot reproduce this decision table.
    use super::{anonymous_me_response, sanitized_next};
    use axum::http::StatusCode;

    #[test]
    fn relative_paths_accepted() {
        assert_eq!(
            sanitized_next(Some("/dashboard")).as_deref(),
            Some("/dashboard")
        );
        assert_eq!(
            sanitized_next(Some("/a/b?x=1")).as_deref(),
            Some("/a/b?x=1")
        );
    }

    #[test]
    fn allowlisted_absolute_hosts_accepted() {
        assert_eq!(
            sanitized_next(Some("https://auto.ml/x")).as_deref(),
            Some("https://auto.ml/x")
        );
        assert_eq!(
            sanitized_next(Some("https://scholar.auto.ml/y")).as_deref(),
            Some("https://scholar.auto.ml/y")
        );
    }

    #[test]
    fn open_redirects_and_foreign_hosts_rejected() {
        assert_eq!(sanitized_next(Some("//evil.com")), None);
        assert_eq!(sanitized_next(Some("https://evil.com/")), None);
        assert_eq!(sanitized_next(Some("evil.com")), None);
        assert_eq!(sanitized_next(Some("")), None);
        assert_eq!(sanitized_next(None), None);
        assert_eq!(
            sanitized_next(Some(&format!("/{}", "a".repeat(2048)))),
            None
        );
    }

    #[test]
    fn backslash_trick_is_accepted_divergence_from_validate_next() {
        // DELIBERATE pin: automl currently accepts `/\host`; w-id-axum rejects it.
        // Documents why routes() would change wire behavior here.
        assert_eq!(
            sanitized_next(Some("/\\evil.com")).as_deref(),
            Some("/\\evil.com")
        );
    }

    #[test]
    fn auth_me_uses_status_code_for_missing_sso_config() {
        let response = anonymous_me_response(StatusCode::SERVICE_UNAVAILABLE, false);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn auth_me_uses_401_for_anonymous_session() {
        let response = anonymous_me_response(StatusCode::UNAUTHORIZED, true);
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
