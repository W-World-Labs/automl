use std::{net::SocketAddr, sync::Arc};

use automl_server::{build_router, build_state_lazy, load_runtime, run_migrations, storage_init};
use axum::middleware;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    automl_server::ensure_rustls_crypto_provider();
    dotenvy::dotenv().ok();

    // `--print-openapi`: emit the spec (as served at /api/v1/openapi.json) and
    // exit. Needs no database/env — this is what web type generation and the
    // CI drift gate consume, so it must stay dependency-free.
    if std::env::args().any(|a| a == "--print-openapi") {
        println!("{}", automl_server::openapi_json()?);
        return Ok(());
    }

    // Install tracing first. When `W_RADAR_CODE` is set, xradar attaches a
    // TracingBridge that exports spans to Radar; without the code we still get
    // local fmt logs (never silent-fail the process).
    xradar::init_tracing("automl-server");

    let (cfg, sso_config) = load_runtime()?;
    let enrichment_provider = if cfg.paper_enrichment.daas_openalex_base.is_some() {
        "daas-openalex"
    } else if cfg.paper_enrichment.openalex_mode.as_deref() == Some("public") {
        "openalex-public"
    } else {
        "stub"
    };
    tracing::info!(
        paper_enrichment = enrichment_provider,
        w_papers_title_cache = cfg.paper_enrichment.w_papers_base.is_some(),
        "Research Hub paper enrichment configured"
    );
    let bind_addr = cfg.bind_addr;
    let production = cfg.production;
    let dev_admin = cfg.dev_admin_enabled();

    // Radar HTTP middleware + capabilities. Client is `None` only when the
    // tracking code env is missing — log loudly so gate_on_code-style silence
    // is visible to operators (CODE must come from the materialized Radar KV
    // environment projection).
    let radar_client = xradar::XRadarClient::from_env().map(Arc::new);
    if radar_client.is_some() {
        tracing::info!("Radar telemetry configured (W_RADAR_CODE)");
    } else {
        tracing::warn!(
            "Radar telemetry disabled: set W_RADAR_CODE (canonical: W-W-AUTOML-C06A4011) \
             via the materialized Radar env KV projection"
        );
    }

    let storage = storage_init::build_storage(&cfg).await?;

    // Lazy pool: the server boots and serves /health (reporting the database
    // as degraded) even when PostgreSQL is still coming up.
    let state = build_state_lazy(cfg, sso_config, storage)?;

    // Migrations are a startup prerequisite. Serving against an unapplied or
    // partially upgraded schema produces misleading health and broken APIs.
    run_migrations(&state.db).await?;
    tracing::info!("database migrations applied");

    // Compensation sweep: papers stuck in repo_pending (creation-time attempt
    // failed or the process restarted) get their Gitea repo provisioned here.
    let _gitea_sweeper = state.gitea.clone().map(|client| {
        let db = state.db.clone();
        w_task::Periodic::every(std::time::Duration::from_secs(env_secs(
            "GITEA_SWEEP_SECS",
            60,
        )))
        .named("gitea-repo-provision")
        .skip_first()
        .spawn(move || {
            let db = db.clone();
            let client = client.clone();
            async move { automl_server::domain::gitea::provision_pending(&db, &client).await }
        })
    });

    // Outbound webhook pipeline: fan out the transactional outbox and attempt
    // due deliveries. All external calls happen here, never on request paths.
    let _webhook_worker = {
        let db = state.db.clone();
        let http = w_http::client_with_timeout(std::time::Duration::from_secs(10));
        w_task::Periodic::every(std::time::Duration::from_secs(env_secs(
            "WEBHOOK_TICK_SECS",
            30,
        )))
        .named("webhook-delivery")
        .skip_first()
        .spawn(move || {
            let db = db.clone();
            let http = http.clone();
            async move { automl_server::domain::events::run_once(&db, &http).await }
        })
    };

    // Timeout sweep: registered runs whose evidence never arrives go `stale`
    // (see docs/STATE-MACHINES.md).
    let _stale_sweeper = {
        let db = state.db.clone();
        let hours = env_secs("RUN_STALE_HOURS", 72) as i64;
        w_task::Periodic::every(std::time::Duration::from_secs(3600))
            .named("run-stale-sweep")
            .skip_first()
            .spawn(move || {
                let db = db.clone();
                async move {
                    automl_server::domain::reproduce::mark_stale_runs(&db, hours)
                        .await
                        .map(|_| ())
                }
            })
    };

    // Capabilities heartbeat so Radar sees the process as alive (project_id
    // must match the platform registry id `w-automl`).
    let _radar_heartbeat = radar_client.as_ref().map(|client| {
        let capabilities = xradar::Capabilities::new("w-automl")
            .runtime("rust")
            .sdk_version("xradar", env!("CARGO_PKG_VERSION"))
            .git_from_env();
        xradar::XRadarClient::spawn_capabilities_heartbeat(client.clone(), Arc::new(capabilities))
    });

    let mut app = build_router(state);
    if let Some(client) = radar_client.as_ref() {
        // Outer layer: records HTTP performance_span events to Radar /collect.
        app = app.layer(middleware::from_fn_with_state(
            (**client).clone(),
            xradar::track_axum_request,
        ));
    }

    tracing::info!(
        addr = %bind_addr,
        production,
        dev_admin,
        radar = radar_client.is_some(),
        "automl-server v{} listening",
        env!("CARGO_PKG_VERSION"),
    );
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(w_web_kit::shutdown_signal())
    .await?;
    Ok(())
}

/// Operational tunable read once at startup (worker cadence overrides).
fn env_secs(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
