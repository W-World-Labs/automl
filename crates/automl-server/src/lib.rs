//! Auto.ml API server library — binary entry is `main.rs`.
//!
//! Clean architecture layout:
//! - `domain` — state machines, evidence rules, SQL (no axum)
//! - `api` — HTTP handlers only
//! - `auth` — SSO / keys / scopes / rate limit
//! - `storage_init` — w-storage factory

pub mod api;
pub mod auth;
pub mod config;
pub mod daas;
pub mod domain;
pub mod error;
pub mod paper_enrichment;
pub mod seo;
pub mod state;
pub mod storage_init;

/// Select the fleet-standard Rustls ring provider before any HTTP/TLS client
/// is constructed. Calling this repeatedly is harmless and lets integration
/// tests exercise the same initialization contract as the production binary.
pub fn ensure_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

use std::{net::SocketAddr, sync::Arc, time::Duration, time::Instant};

use axum::{extract::DefaultBodyLimit, Router};
use sqlx::postgres::PgPoolOptions;
use tower_http::{compression::CompressionLayer, timeout::TimeoutLayer, trace::TraceLayer};
use w_id_sdk::SsoConfig;
use w_storage::Storage;

use crate::{
    auth::{rate_limit, sso},
    config::Config,
    state::AppState,
};

const PRE_V2_ARCHIVE_SCHEMA: &str = "automl_pre_v2";

/// Construct application state (DB pool + config + optional SSO + storage).
pub async fn build_state(
    cfg: Config,
    sso_config: Option<SsoConfig>,
    storage: Arc<dyn Storage>,
) -> anyhow::Result<AppState> {
    ensure_rustls_crypto_provider();
    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await?;

    run_migrations(&db).await?;

    let gitea = gitea_client(&cfg);
    let paper_enrichment = Arc::new(paper_enrichment::PaperEnrichment::new(
        &cfg.paper_enrichment,
    ));
    let daas = Arc::new(daas::Daas::new(&cfg.daas));
    Ok(AppState {
        db,
        cfg: Arc::new(cfg),
        started_at: Instant::now(),
        sso: sso_config,
        storage,
        gitea,
        paper_enrichment,
        daas,
    })
}

fn gitea_client(cfg: &Config) -> Option<Arc<domain::gitea::GiteaClient>> {
    cfg.gitea
        .clone()
        .map(|g| Arc::new(domain::gitea::GiteaClient::new(g)))
}

/// Lazy pool variant used by the binary so `/health` can report degraded DB.
pub fn build_state_lazy(
    cfg: Config,
    sso_config: Option<SsoConfig>,
    storage: Arc<dyn Storage>,
) -> anyhow::Result<AppState> {
    ensure_rustls_crypto_provider();
    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy(&cfg.database_url)?;

    let gitea = gitea_client(&cfg);
    let paper_enrichment = Arc::new(paper_enrichment::PaperEnrichment::new(
        &cfg.paper_enrichment,
    ));
    let daas = Arc::new(daas::Daas::new(&cfg.daas));
    Ok(AppState {
        db,
        cfg: Arc::new(cfg),
        started_at: Instant::now(),
        sso: sso_config,
        storage,
        gitea,
        paper_enrichment,
        daas,
    })
}

/// Run pending migrations against an existing pool (binary startup path).
///
/// The pre-v2 service used a separate, text-keyed schema with several table
/// names that v2 also owns. It did not record those tables in SQLx migration
/// history. When that exact legacy fingerprint is present, preserve it
/// transactionally in `automl_pre_v2` before applying the v2 migrations.
/// Unknown or partially converted schemas fail closed instead of guessing.
pub async fn run_migrations(db: &sqlx::PgPool) -> anyhow::Result<()> {
    prepare_pre_v2_schema(db).await?;
    sqlx::migrate!("./migrations").run(db).await?;
    Ok(())
}

async fn prepare_pre_v2_schema(db: &sqlx::PgPool) -> anyhow::Result<()> {
    let mut tx = db.begin().await?;
    // Serialize the one-time compatibility step across concurrent starts.
    sqlx::query("SELECT pg_advisory_xact_lock(8672020001)")
        .execute(&mut *tx)
        .await?;

    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(&mut *tx)
        .await?;
    let scholars_exists: bool =
        sqlx::query_scalar("SELECT to_regclass(format('%I.%I', $1, 'scholars')) IS NOT NULL")
            .bind(&schema)
            .fetch_one(&mut *tx)
            .await?;
    if !scholars_exists {
        tx.commit().await?;
        return Ok(());
    }

    let columns: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT column_name, data_type
           FROM information_schema.columns
           WHERE table_schema = $1 AND table_name = 'scholars'"#,
    )
    .bind(&schema)
    .fetch_all(&mut *tx)
    .await?;
    let has_column = |name: &str, data_type: &str| {
        columns
            .iter()
            .any(|(column, ty)| column == name && ty == data_type)
    };
    let v2_schema = has_column("id", "uuid")
        && has_column("w_id", "text")
        && has_column("display_name", "text");
    if v2_schema {
        tx.commit().await?;
        return Ok(());
    }

    let legacy_papers: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1
               FROM information_schema.columns
               WHERE table_schema = $1 AND table_name = 'papers'
                 AND column_name = 'journal_slug' AND data_type = 'text'
           )"#,
    )
    .bind(&schema)
    .fetch_one(&mut *tx)
    .await?;
    let legacy_schema = has_column("id", "text")
        && has_column("name", "text")
        && !columns.iter().any(|(column, _)| column == "w_id")
        && legacy_papers;
    anyhow::ensure!(
        legacy_schema,
        "refusing to migrate an unknown or partially converted AutoML schema"
    );

    let archive_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM information_schema.schemata WHERE schema_name = $1)",
    )
    .bind(PRE_V2_ARCHIVE_SCHEMA)
    .fetch_one(&mut *tx)
    .await?;
    anyhow::ensure!(
        !archive_exists,
        "legacy archive schema {PRE_V2_ARCHIVE_SCHEMA} already exists while legacy tables remain public"
    );

    // The only dynamic identifier here is a compile-time constant, quoted by
    // `quote_identifier`; SQLx 0.9 requires an explicit audit marker.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE SCHEMA {}",
        quote_identifier(PRE_V2_ARCHIVE_SCHEMA)
    )))
    .execute(&mut *tx)
    .await?;
    let tables: Vec<String> = sqlx::query_scalar(
        r#"SELECT table_name
           FROM information_schema.tables
           WHERE table_schema = $1 AND table_type = 'BASE TABLE'
             AND table_name <> '_sqlx_migrations'
           ORDER BY table_name"#,
    )
    .bind(&schema)
    .fetch_all(&mut *tx)
    .await?;
    anyhow::ensure!(
        !tables.is_empty(),
        "legacy schema contained no tables to archive"
    );
    for table in tables {
        let statement = format!(
            "ALTER TABLE {}.{} SET SCHEMA {}",
            quote_identifier(&schema),
            quote_identifier(&table),
            quote_identifier(PRE_V2_ARCHIVE_SCHEMA),
        );
        // Both identifiers originate from PostgreSQL's catalog and are
        // double-quoted with embedded quotes escaped before interpolation.
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// Assemble the full HTTP router (OpenAPI + `/api/v1` + rate limits + middleware).
pub fn build_router(state: AppState) -> Router {
    ensure_rustls_crypto_provider();
    let cfg = state.cfg.clone();
    let (v1_router, openapi) = api::v1(&cfg);

    let mut doc = serde_json::to_value(&openapi).expect("openapi serializes");
    if let Some(paths) = doc.get_mut("paths").and_then(|p| p.as_object_mut()) {
        let rewritten: serde_json::Map<String, serde_json::Value> = paths
            .iter()
            .map(|(k, v)| (format!("/api/v1{k}"), v.clone()))
            .collect();
        *paths = rewritten;
    }

    let origins: Vec<String> = cfg
        .allowed_origins
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Write governor is innermost (runs first on the way in) so write paths
    // hit the stricter bucket before the general one.
    let v1 = v1_router
        .layer(rate_limit::write_governor_layer(&cfg.rate_limit))
        .layer(rate_limit::governor_layer(&cfg.rate_limit));

    Router::new()
        .merge(w_web_kit::openapi_routes(doc))
        .nest("/api/v1", v1)
        // Root-level: W_ID_REDIRECT_URI, the SPA and the CLI loopback bounce
        // all address /auth/{login,callback,me,logout} (v1-compatible paths).
        .merge(sso::sso_router())
        .route("/robots.txt", axum::routing::get(seo::robots_txt))
        .route("/sitemap.xml", axum::routing::get(seo::sitemap_xml))
        .route("/llms.txt", axum::routing::get(seo::llms_txt))
        .fallback(spa_fallback)
        .with_state(state)
        .layer(axum::middleware::from_fn(w_web_kit::request_id_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(w_web_kit::cors_from_origins_credentials(&origins))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(DefaultBodyLimit::max(cfg.max_body_size))
}

/// Embedded web frontend (`web/dist`, built by `npm run build` before the
/// release compile). An empty dist (dev checkout) yields honest 404s — never
/// a stub index.html (v1 defect class).
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist/"]
#[exclude = ".gitkeep"]
struct WebAssets;

static SPA_CONFIG: std::sync::LazyLock<w_web_kit::SpaConfig> = std::sync::LazyLock::new(|| {
    // Real server routes must 404 instead of answering HTML.
    w_web_kit::SpaConfig::with_guards(["api", "auth", "openapi.json"])
});

async fn spa_fallback(
    axum::extract::State(state): axum::extract::State<AppState>,
    uri: axum::http::Uri,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // Snapshot routes (home, papers index, paper + thread deep links) get
    // server-rendered head metadata AND a crawler-visible body — AI crawlers
    // and social scrapers don't run JS.
    if seo::route_wants_snapshot(uri.path()) {
        if let Some(shell) = w_web_kit::embedded_asset::<WebAssets>("index.html") {
            if let Ok(bytes) = axum::body::to_bytes(shell.into_body(), 4 * 1024 * 1024).await {
                let shell = String::from_utf8_lossy(&bytes);
                if let Some(html) = seo::render_route(&state, uri.path(), &shell).await {
                    return (
                        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        html,
                    )
                        .into_response();
                }
            }
        }
    }
    w_web_kit::spa_response::<WebAssets>(&uri, &SPA_CONFIG)
}

/// The OpenAPI document exactly as served at `/api/v1/openapi.json`
/// (path-prefixed), independent of any runtime environment. Consumed by
/// `--print-openapi` for frontend type generation and the CI drift gate.
pub fn openapi_json() -> anyhow::Result<String> {
    let cfg = Config {
        bind_addr: "127.0.0.1:0".parse()?,
        database_url: String::new(),
        allowed_origins: String::new(),
        max_body_size: 0,
        production: true,
        allow_dev_admin: false,
        // Values are irrelevant (no server runs) but must be constructible.
        rate_limit: crate::config::RateLimitConfig {
            rps: 1,
            burst: 1,
            write_rps: 1,
            write_burst: 1,
            feedback_rps: 1,
            feedback_burst: 1,
        },
        storage_backend: String::new(),
        storage_local_root: String::new(),
        storage_public_base: String::new(),
        gitea: None,
        cli_dist_base: "https://dist.example.test".into(),
        public_base_url: "https://accept.example.test".into(),
        paper_enrichment: crate::config::EnrichmentConfig::default(),
        daas: crate::config::DaasConfig::default(),
    };
    let (_router, openapi) = api::v1(&cfg);
    let mut doc = serde_json::to_value(&openapi)?;
    if let Some(paths) = doc.get_mut("paths").and_then(|p| p.as_object_mut()) {
        let rewritten: serde_json::Map<String, serde_json::Value> = paths
            .iter()
            .map(|(k, v)| (format!("/api/v1{k}"), v.clone()))
            .collect();
        *paths = rewritten;
    }
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// Load config + optional SSO from the environment.
pub fn load_runtime() -> anyhow::Result<(Config, Option<SsoConfig>)> {
    let cfg = Config::from_env()?;
    let sso_config = sso::sso_config_from_env();
    Ok((cfg, sso_config))
}

/// Bind address type re-export for main.
pub type BindAddr = SocketAddr;
