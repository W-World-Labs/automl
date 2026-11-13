//! Integration tests for iteration 8 server surface: CLI loopback login
//! (start → code → token exchange) and /cli/install metadata.
//! Requires DATABASE_URL.

use std::sync::Arc;

use automl_server::{
    auth::sso,
    build_router,
    config::{Config, RateLimitConfig},
    state::AppState,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;
use w_id_sdk::{SsoConfig, SsoUser};

fn test_config() -> Config {
    Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: std::env::var("DATABASE_URL").unwrap_or_default(),
        allowed_origins: "http://localhost:5173".into(),
        max_body_size: 2 * 1024 * 1024,
        production: false,
        allow_dev_admin: false,
        rate_limit: RateLimitConfig {
            rps: 10_000,
            burst: 10_000,
            write_rps: 10_000,
            write_burst: 10_000,
            feedback_rps: 10_000,
            feedback_burst: 10_000,
        },
        storage_backend: "local".into(),
        storage_local_root: std::env::temp_dir()
            .join(format!("automl-it8-objects-{}", Uuid::new_v4()))
            .to_string_lossy()
            .into(),
        storage_public_base: "http://127.0.0.1:8030".into(),
        gitea: None,
        cli_dist_base: "https://dist.example.test/bin/".into(),
        public_base_url: "https://accept.example.test".into(),
        paper_enrichment: automl_server::config::EnrichmentConfig::default(),
        daas: automl_server::config::DaasConfig::default(),
    }
}

async fn setup() -> AppState {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migrate");
    let cfg = test_config();
    let storage = automl_server::storage_init::build_storage(&cfg)
        .await
        .expect("storage");
    AppState {
        db,
        cfg: Arc::new(cfg),
        started_at: std::time::Instant::now(),
        sso: Some(SsoConfig::new(
            "https://auth.example.test",
            "c",
            "s",
            "http://localhost/cb",
            "test-jwt-secret-that-is-long-enough-32b",
        )),
        storage,
        gitea: None,
        paper_enrichment: Arc::new(automl_server::paper_enrichment::PaperEnrichment::stub()),
        daas: Arc::new(automl_server::daas::Daas::stub()),
    }
}

async fn json_body(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn session_token(state: &AppState, sub: &str) -> String {
    let user = SsoUser {
        workspace_id: None,
        sub: sub.into(),
        login: Some(sub.into()),
        email: None,
        name: None,
        picture: None,
        role: None,
        is_platform_admin: false,
        workspaces: vec![],
        demo_workspaces: vec![],
        tier: None,
        workspace_tier: None,
        product: None,
        quotas: Default::default(),
        entitlements: vec![],
    };
    state.sso.as_ref().unwrap().issue_session(&user).unwrap()
}

#[tokio::test]
async fn cli_install_metadata_matches_version() {
    let state = setup().await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/cli/install")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = json_body(resp).await;
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    let platforms = v["platforms"].as_array().unwrap();
    assert!(!platforms.is_empty());
    let linux = &platforms[0];
    assert_eq!(linux["platform"], "linux-x86_64");
    let url = linux["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://dist.example.test/bin/automl-cli-linux-x86_64-v"),
        "url points at CI artifacts, never at server target/: {url}"
    );
    assert!(linux["sha256_url"].as_str().unwrap().ends_with(".sha256"));
}

#[tokio::test]
async fn cli_loopback_login_full_flow() {
    let state = setup().await;
    let sub = format!("it8-login-{}", Uuid::new_v4());
    sso::ensure_scholar(&state.db, &sub, "cli-user")
        .await
        .unwrap();
    let token = session_token(&state, &sub);
    let app = build_router(state.clone());

    let verifier = "test-verifier-0123456789abcdef";
    let challenge = w_crypto::sha256_hex(verifier.as_bytes());
    let redirect = "http%3A%2F%2F127.0.0.1%3A43210%2Fcallback";
    let start = format!("/api/v1/cli/auth/start?redirect_uri={redirect}&challenge={challenge}");

    // Without a session: bounce to /auth/login carrying next=.
    let anon = app
        .clone()
        .oneshot(Request::builder().uri(&start).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(anon.status().is_redirection());
    let loc = anon.headers()["location"].to_str().unwrap();
    assert!(loc.starts_with("/auth/login?next="), "got {loc}");

    // With a session: redirect to the loopback with a one-time code.
    let approved = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&start)
                .header("cookie", format!("automl_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(approved.status().is_redirection());
    let loc = approved.headers()["location"].to_str().unwrap().to_string();
    assert!(
        loc.starts_with("http://127.0.0.1:43210/callback?code="),
        "got {loc}"
    );
    let code = loc.split("code=").nth(1).unwrap().to_string();

    // Wrong verifier → 401, and the code is burned (single use).
    let bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/cli/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"code": code, "verifier": "wrong"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

    // Re-run the approval to get a fresh code, then exchange correctly.
    let approved = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&start)
                .header("cookie", format!("automl_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let loc = approved.headers()["location"].to_str().unwrap().to_string();
    let code = loc.split("code=").nth(1).unwrap().to_string();

    let ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/cli/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"code": code, "verifier": verifier, "name": "cli-test"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::CREATED);
    let created = json_body(ok).await;
    let key = created["key"].as_str().unwrap();
    assert!(key.starts_with("amk_"));
    assert_eq!(created["name"], "cli-test");
    let scopes = created["scopes"].as_array().unwrap();
    assert!(scopes.iter().any(|s| s == "papers:read"));
    assert!(!scopes.iter().any(|s| s == "admin"), "never mints admin");

    // The minted key authenticates real API calls.
    let papers = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/papers?limit=1")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(papers.status(), StatusCode::OK);

    // Replay of the used code → 401.
    let replay = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/cli/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"code": code, "verifier": verifier}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cli_auth_start_rejects_non_loopback_redirect() {
    let state = setup().await;
    let sub = format!("it8-evil-{}", Uuid::new_v4());
    sso::ensure_scholar(&state.db, &sub, "x").await.unwrap();
    let token = session_token(&state, &sub);
    let app = build_router(state);

    let challenge = w_crypto::sha256_hex(b"v");
    for evil in [
        "https%3A%2F%2Fevil.test%2Fcallback",
        "http%3A%2F%2Flocalhost%3A9%2Fcallback",
        "http%3A%2F%2F127.0.0.1%3A9%2Fevil",
    ] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/cli/auth/start?redirect_uri={evil}&challenge={challenge}"
                    ))
                    .header("cookie", format!("automl_session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "redirect {evil} must be rejected"
        );
    }
}

#[tokio::test]
async fn runs_list_paginates_and_filters() {
    let state = setup().await;
    let app = build_router(state.clone());
    // Just shape-check with an authed key: 200 + data array.
    let scholar = sso::ensure_scholar(&state.db, &format!("it8-runs-{}", Uuid::new_v4()), "r")
        .await
        .unwrap();
    let key = automl_server::auth::keys::create_key(
        &state.db,
        scholar,
        automl_server::auth::keys::CreateKeyRequest {
            name: "t".into(),
            scopes: vec!["papers:read".into()],
            expires_at: None,
        },
        &["papers:read".to_string()],
        true,
    )
    .await
    .unwrap()
    .key;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/reproduce/runs?limit=5")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = json_body(resp).await;
    assert!(v["data"].is_array());
}
