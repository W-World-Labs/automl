//! Dev-admin backdoor — dual-gated (`ALLOW_DEV_ADMIN=1` ∧ non-production).
//!
//! When disabled the route is **not registered** (clients see 404 from the
//! router), never a soft 401 that would reveal the path exists.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;
use w_id_axum::build_cookie;
use w_id_sdk::SsoUser;

use crate::{
    auth::sso::{self, SESSION_COOKIE},
    error::{ApiError, ApiResult},
    state::AppState,
};

const DEV_ADMIN_SUB: &str = "dev-admin";
const DEV_ADMIN_LOGIN: &str = "dev-admin";

#[derive(Serialize, utoipa::ToSchema)]
pub struct DevAdminResponse {
    pub scholar_id: uuid::Uuid,
    pub w_id: String,
    pub scopes: Vec<String>,
    pub message: String,
}

/// Mint a platform-admin session cookie for local development.
#[utoipa::path(
    post,
    path = "/auth/dev-admin",
    tag = "auth",
    responses(
        (status = 200, description = "Dev admin session issued", body = DevAdminResponse),
        (status = 503, description = "SSO not configured"),
    )
)]
pub async fn dev_admin(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    // Defense in depth: even if somehow mounted, refuse in production.
    if !state.cfg.dev_admin_enabled() {
        return Err(ApiError::not_found("not found"));
    }
    let sso = state
        .sso
        .as_ref()
        .ok_or_else(|| ApiError::unavailable("SSO is not configured"))?;

    let user = SsoUser {
        workspace_id: None,
        sub: DEV_ADMIN_SUB.into(),
        login: Some(DEV_ADMIN_LOGIN.into()),
        email: None,
        name: Some("Dev Admin".into()),
        picture: None,
        role: None,
        is_platform_admin: true,
        workspaces: vec![],
        demo_workspaces: vec![],
        tier: None,
        workspace_tier: None,
        product: None,
        quotas: Default::default(),
        entitlements: vec![],
    };

    let scholar_id = sso::upsert_scholar(&state.db, &user).await?;
    let token = sso.issue_session(&user).map_err(|e| {
        tracing::error!(error = %e, "dev-admin issue_session failed");
        ApiError::unavailable("failed to issue session")
    })?;

    let cookie = build_cookie(SESSION_COOKIE, token);
    let body = DevAdminResponse {
        scholar_id,
        w_id: DEV_ADMIN_SUB.into(),
        scopes: {
            let mut s: Vec<String> = automl_core::scope::SCHOLAR_DEFAULT
                .iter()
                .map(|x| (*x).to_string())
                .collect();
            s.push(automl_core::scope::ADMIN.to_string());
            s.sort();
            s
        },
        message: "dev-admin session cookie set".into(),
    };

    Ok(([(http::header::SET_COOKIE, cookie.to_string())], Json(body)))
}
