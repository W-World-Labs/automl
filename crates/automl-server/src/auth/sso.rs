//! W-ID SSO wiring via fleet `w-id-axum` + minimal scholar provisioning.

use std::sync::Arc;

use w_id_axum::{FlowError, SsoApp, SsoSettings};
use w_id_sdk::{SsoConfig, SsoError, SsoUser};

use crate::state::AppState;

pub const SESSION_COOKIE: &str = "automl_session";
pub const STATE_COOKIE: &str = "automl_oauth_state";
pub const ROUTE_PREFIX: &str = "/auth";

/// Load SSO config from the standard W-ID env vars. Missing config is non-fatal
/// (API keys still work; `/auth/*` returns 503).
pub fn sso_config_from_env() -> Option<SsoConfig> {
    match SsoConfig::from_env() {
        Ok(cfg) => {
            tracing::info!("W-ID SSO enabled");
            Some(cfg)
        }
        Err(SsoError::MissingEnv(var)) => {
            tracing::info!(
                missing = var,
                "W-ID SSO not configured; /api/v1/auth/* will return 503"
            );
            None
        }
        Err(err) => {
            tracing::warn!(?err, "W-ID SSO configuration invalid; disabling SSO");
            None
        }
    }
}

pub fn sso_settings() -> SsoSettings {
    SsoSettings::new(ROUTE_PREFIX)
        .cookies(SESSION_COOKIE, STATE_COOKIE)
        .flow_error(FlowError::Json)
        .redirect("/")
        // Same-origin relative `next` only — used by the CLI loopback login
        // (`/auth/login?next=/api/v1/cli/auth/start?...`) and the web app's
        // return-to. Absolute URLs are rejected by the policy.
        .allow_next(w_id_axum::NextPolicy::SameOrigin)
}

impl SsoApp for AppState {
    fn sso(&self) -> Option<&SsoConfig> {
        self.sso.as_ref()
    }

    /// Provision a minimal scholar row on first login. Non-fatal: login proceeds
    /// even if the upsert fails (logged).
    async fn on_login(&self, user: &SsoUser) {
        if let Err(err) = upsert_scholar(&self.db, user).await {
            tracing::warn!(?err, sub = %user.sub, "failed to upsert scholar on login");
        }
    }
}

pub async fn upsert_scholar(db: &sqlx::PgPool, user: &SsoUser) -> sqlx::Result<uuid::Uuid> {
    let display = user
        .name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| user.username());

    let id: uuid::Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO scholars (w_id, display_name)
        VALUES ($1, $2)
        ON CONFLICT (w_id) DO UPDATE
            SET display_name = EXCLUDED.display_name,
                updated_at   = now()
        RETURNING id
        "#,
    )
    .bind(&user.sub)
    .bind(&display)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn scholar_id_for_w_id(
    db: &sqlx::PgPool,
    w_id: &str,
) -> sqlx::Result<Option<uuid::Uuid>> {
    sqlx::query_scalar("SELECT id FROM scholars WHERE w_id = $1")
        .bind(w_id)
        .fetch_optional(db)
        .await
}

/// Convenience for tests / dev-admin: ensure a scholar exists and return its id.
pub async fn ensure_scholar(
    db: &sqlx::PgPool,
    w_id: &str,
    display_name: &str,
) -> sqlx::Result<uuid::Uuid> {
    sqlx::query_scalar(
        r#"
        INSERT INTO scholars (w_id, display_name)
        VALUES ($1, $2)
        ON CONFLICT (w_id) DO UPDATE
            SET display_name = EXCLUDED.display_name,
                updated_at   = now()
        RETURNING id
        "#,
    )
    .bind(w_id)
    .bind(display_name)
    .fetch_one(db)
    .await
}

/// Mount fleet SSO routes. State type is `AppState` (Clone + SsoApp).
pub fn sso_router() -> axum::Router<AppState> {
    w_id_axum::routes::<AppState>(sso_settings())
}

// Re-export for callers that hold Arc<AppState> — blanket impl covers Arc.
#[allow(dead_code)]
pub type ArcState = Arc<AppState>;
