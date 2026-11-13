// ============================================================
// automl-server — Main entry point
// Axum HTTP server for Auto.ml platform
// ============================================================

mod auth;
mod config;
mod db;
mod error;
mod models;
mod openapi;
mod reproduce;
mod routes;
mod spa;
mod storage;

use axum::extract::DefaultBodyLimit;
use axum::{
    middleware,
    routing::get_service,
    Router,
};
use std::sync::Arc;
use tower_http::services::ServeFile;
use tower_http::trace::TraceLayer;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    xradar::init_tracing("automl-server");

    w_self_update::check_update(w_self_update::UpdateConfig {
        repo: "W-World-Labs/automl",
        current_version: env!("CARGO_PKG_VERSION"),
        binary_name: "automl-server-linux-amd64",
        ..Default::default()
    });

    // Load config (also loads .env via dotenvy)
    let cfg = config::Config::from_env();
    cfg.validate()?;
    tracing::info!(
        "Starting automl-server on {} [env: {}]",
        cfg.bind_addr,
        cfg.env_name
    );

    let sso = match w_id_sdk::SsoConfig::from_env() {
        Ok(config) => {
            tracing::info!("W-ID SSO configured");
            Some(config)
        }
        Err(error) if cfg.is_production() => {
            anyhow::bail!("W-ID SSO is required in production: {error}");
        }
        Err(error) => {
            tracing::warn!("W-ID SSO disabled for this run: {}", error);
            None
        }
    };

    let radar_client = xradar::XRadarClient::from_env().map(Arc::new);
    if radar_client.is_some() {
        tracing::info!("Radar telemetry configured");
    } else {
        tracing::warn!("Radar telemetry disabled: W_RADAR_CODE not set");
    }

    // Initialize database
    let pool = db::init_pool(&cfg.database_url).await?;
    db::run_migrations(&pool).await?;
    tracing::info!("Database ready");

    // Initialize R2/S3 storage
    let storage = storage::Storage::new(&cfg).await;
    tracing::info!("Storage connected (bucket: {})", cfg.r2_bucket);

    // Build CORS layer based on environment.
    // Shared w-web-kit env-allowlist builder (faithful superset of the prior
    // local `build_cors`): reads ALLOWED_ORIGINS (default "*"), wildcard/empty →
    // Any, else a parsed origin list; methods/headers Any; no credentials.
    let cors = w_web_kit::cors::cors_from_env("ALLOWED_ORIGINS", "*");
    tracing::info!(
        "CORS: {}",
        if cfg.allowed_origins == "*" {
            "allow all (dev)".into()
        } else {
            cfg.allowed_origins.clone()
        }
    );

    if cfg.admin_keys.is_empty() {
        if cfg.dev_admin_elevation_enabled() {
            tracing::warn!(
                "No ADMIN_KEYS; ALLOW_DEV_ADMIN=1 elevates authenticated users (dev only)"
            );
        } else {
            tracing::warn!(
                "No ADMIN_KEYS configured — admin endpoints require admin/platform trust_level (fail-closed; set ALLOW_DEV_ADMIN=1 only for local dev elevation)"
            );
        }
    } else {
        tracing::info!("Admin keys: {} configured", cfg.admin_keys.len());
    }

    // Build shared state
    let started_at = std::time::Instant::now();
    let state = routes::AppState {
        db: pool,
        storage,
        config: cfg.clone(),
        sso,
        radar_configured: radar_client.is_some(),
        started_at,
    };

    if let Some(client) = radar_client.as_ref() {
        // Radar registry project_id (fleet rename 2026-06-15: automl → w-automl);
        // the old id FK-fails on /api/platform/runtime/capabilities every heartbeat.
        let capabilities = xradar::Capabilities::new("w-automl")
            .runtime("rust")
            .sdk_version("w_id_sdk", "0.3.0")
            .tenancy(xradar::TenancyState {
                mirror_installed: true,
                auth_callback_wires_mirror: state.sso.is_some(),
                principal_can_access_uses: 1,
                user_owned_tables: vec![
                    "scholars".to_string(),
                    "papers".to_string(),
                    "assets".to_string(),
                    "agent_devices".to_string(),
                ],
            })
            .git_from_env();
        let _heartbeat = xradar::XRadarClient::spawn_capabilities_heartbeat(
            client.clone(),
            Arc::new(capabilities),
        );
    }

    // Start the reproduction worker (claims queued reproduction_jobs and runs
    // them in the configured sandbox backend). Cloned before the router takes
    // ownership of `state`.
    reproduce::spawn_worker(state.clone());

    // Build router with security layers
    let mut app = Router::new()
        .merge(w_web_kit::openapi_routes(openapi::document()))
        .merge(routes::root_router())
        .nest("/api/v1", routes::api_router())
        .route_service(
            "/skills/automl.md",
            get_service(ServeFile::new("./skills/automl.md")),
        )
        .route_service(
            "/skills/automl.manifest.json",
            get_service(ServeFile::new("./skills/automl.manifest.json")),
        )
        // Embedded SPA — no disk dist required at runtime
        .fallback(spa::spa_handler)
        .with_state(state)
        .layer(cors)
        .layer(DefaultBodyLimit::max(cfg.max_body_size))
        .layer(TraceLayer::new_for_http());

    if let Some(client) = radar_client.as_ref() {
        app = app.layer(middleware::from_fn_with_state(
            (**client).clone(),
            xradar::track_axum_request,
        ));
    }

    tracing::info!("Frontend embedded in binary (rust-embed)");

    // Start server
    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!("🚀 automl-server listening on http://{}", cfg.bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}
