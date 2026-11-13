//! CLI surface: distribution metadata + loopback login (code → API key).
//!
//! Distribution points at CI-published artifacts on the release host — the server
//! never reads its own target/ directory (v1 defect class).

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{cli_auth, keys::ApiKeyCreated, sso::SESSION_COOKIE},
    error::{ApiError, ApiResult},
    state::AppState,
};

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CliPlatform {
    /// e.g. `linux-x86_64`
    pub platform: String,
    /// Direct download URL of the CI-built binary for the current version.
    pub url: String,
    /// Sidecar file containing the artifact's SHA-256 (hex).
    pub sha256_url: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CliInstallInfo {
    /// Workspace version these artifacts were built from (matches /health).
    pub version: String,
    pub platforms: Vec<CliPlatform>,
}

/// Platforms CI actually publishes today. Extend ONLY together with the
/// release workflow — listing an unpublished platform is a lie (stub-is-501
/// ethos applies to metadata too).
const PUBLISHED_PLATFORMS: &[&str] = &["linux-x86_64"];

#[utoipa::path(
    get, path = "/cli/install", tag = "system",
    responses((status = 200, description = "CLI artifact metadata", body = CliInstallInfo))
)]
pub async fn cli_install(State(state): State<AppState>) -> ApiResult<Json<CliInstallInfo>> {
    let version = env!("CARGO_PKG_VERSION");
    let base = state.cfg.cli_dist_base.trim_end_matches('/');
    let platforms = PUBLISHED_PLATFORMS
        .iter()
        .map(|p| CliPlatform {
            platform: (*p).to_string(),
            url: format!("{base}/automl-cli-{p}-v{version}"),
            sha256_url: format!("{base}/automl-cli-{p}-v{version}.sha256"),
        })
        .collect();
    Ok(Json(CliInstallInfo {
        version: version.to_string(),
        platforms,
    }))
}

// ── loopback login ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CliAuthStartQuery {
    pub redirect_uri: String,
    /// hex(sha256(pkce_verifier))
    pub challenge: String,
}

#[utoipa::path(
    get, path = "/cli/auth/start", tag = "auth",
    params(
        ("redirect_uri" = String, Query, description = "Loopback callback: http://127.0.0.1:<port>/callback"),
        ("challenge" = String, Query, description = "hex(sha256(pkce_verifier))"),
    ),
    responses(
        (status = 303, description = "Redirects to the loopback with ?code=…; or to /auth/login?next=… when no session"),
        (status = 400, description = "Bad redirect_uri or challenge"),
        (status = 503, description = "SSO not configured"),
    )
)]
pub async fn cli_auth_start(
    State(state): State<AppState>,
    Query(q): Query<CliAuthStartQuery>,
    jar: CookieJar,
) -> ApiResult<Response> {
    // Validate BEFORE bouncing through login so a bad CLI request fails fast.
    cli_auth::validate_redirect_uri(&q.redirect_uri)?;
    cli_auth::validate_challenge(&q.challenge)?;

    let Some(sso) = state.sso.as_ref() else {
        return Err(ApiError::unavailable("SSO is not configured"));
    };
    let Some(session) = w_id_axum::read_session(Some(sso), &jar, SESSION_COOKIE) else {
        // Bounce through the browser login and come back here.
        let this = format!(
            "/api/v1/cli/auth/start?redirect_uri={}&challenge={}",
            urlencode(&q.redirect_uri),
            urlencode(&q.challenge)
        );
        return Ok(Redirect::to(&format!("/auth/login?next={}", urlencode(&this))).into_response());
    };

    let scholar_id = match crate::auth::sso::scholar_id_for_w_id(&state.db, &session.sub).await? {
        Some(id) => id,
        None => crate::auth::sso::ensure_scholar(&state.db, &session.sub, &session.login).await?,
    };
    let code = cli_auth::issue_code(&state.db, scholar_id, &q.challenge, &q.redirect_uri).await?;
    Ok(Redirect::to(&format!("{}?code={code}", q.redirect_uri)).into_response())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CliAuthTokenRequest {
    pub code: String,
    /// PKCE verifier whose sha256 hex must equal the challenge from /start.
    pub verifier: String,
    /// Display name for the minted key (default "cli").
    #[serde(default)]
    pub name: Option<String>,
}

#[utoipa::path(
    post, path = "/cli/auth/token", tag = "auth",
    request_body = CliAuthTokenRequest,
    responses(
        (status = 201, description = "API key minted for the CLI (plaintext shown once)", body = ApiKeyCreated),
        (status = 401, description = "Invalid/used/expired code or verifier mismatch"),
    )
)]
pub async fn cli_auth_token(
    State(state): State<AppState>,
    Json(req): Json<CliAuthTokenRequest>,
) -> ApiResult<(axum::http::StatusCode, Json<ApiKeyCreated>)> {
    let created = cli_auth::exchange(
        &state.db,
        &req.code,
        &req.verifier,
        req.name.as_deref().unwrap_or("cli"),
    )
    .await?;
    Ok((axum::http::StatusCode::CREATED, Json(created)))
}
