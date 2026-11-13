//! Integration tests for iteration 2 (auth).
//!
//! Requires `DATABASE_URL` pointing at a Postgres instance that can run
//! migrations (e.g. `postgres://automl:automl_dev_password@127.0.0.1:55439/automl_test`).

use std::{net::SocketAddr, sync::Arc, time::Duration};

use automl_core::scope;
use automl_server::{
    auth::{
        keys::{self, CreateKeyRequest},
        sso,
    },
    build_router,
    config::{Config, RateLimitConfig},
    state::AppState,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration as ChronoDuration, Utc};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;
use w_id_sdk::{SsoConfig, SsoUser};

fn test_config(allow_dev_admin: bool, production: bool, write_burst: u32) -> Config {
    Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: std::env::var("DATABASE_URL").unwrap_or_default(),
        allowed_origins: "http://localhost:5173".into(),
        max_body_size: 2 * 1024 * 1024,
        production,
        allow_dev_admin,
        rate_limit: RateLimitConfig {
            rps: 1000,
            burst: 1000,
            write_rps: 1,
            write_burst,
            feedback_rps: 1000,
            feedback_burst: 1000,
        },
        storage_backend: "local".into(),
        storage_local_root: std::env::temp_dir()
            .join("automl-test-objects")
            .to_string_lossy()
            .into(),
        storage_public_base: "http://127.0.0.1:8030".into(),
        gitea: None,
        cli_dist_base: "https://dist.example.test".into(),
        public_base_url: "https://accept.example.test".into(),
        paper_enrichment: automl_server::config::EnrichmentConfig::default(),
        daas: automl_server::config::DaasConfig::default(),
    }
}

fn test_sso() -> SsoConfig {
    SsoConfig::new(
        "https://auth.example.test",
        "test-client",
        "test-secret",
        "http://localhost/api/v1/auth/callback",
        "test-jwt-secret-that-is-long-enough-32b",
    )
}

async fn setup(allow_dev_admin: bool, production: bool, write_burst: u32) -> AppState {
    let url = std::env::var("DATABASE_URL").expect(
        "DATABASE_URL required for auth integration tests \
         (e.g. postgres://automl:automl_dev_password@127.0.0.1:55439/automl_test)",
    );
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect to DATABASE_URL");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("run migrations");

    let cfg = test_config(allow_dev_admin, production, write_burst);
    let storage = automl_server::storage_init::build_storage(&cfg)
        .await
        .expect("local storage");
    AppState {
        db,
        cfg: Arc::new(cfg),
        started_at: std::time::Instant::now(),
        sso: Some(test_sso()),
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

async fn seed_owner(state: &AppState) -> (Uuid, String) {
    let scholar_id = sso::ensure_scholar(&state.db, "test-owner", "Test Owner")
        .await
        .unwrap();
    let created = keys::create_key(
        &state.db,
        scholar_id,
        CreateKeyRequest {
            name: "owner-key".into(),
            scopes: scope::SCHOLAR_DEFAULT
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            expires_at: None,
        },
        &scope::SCHOLAR_DEFAULT
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>(),
        true,
    )
    .await
    .unwrap();
    (scholar_id, created.key)
}

#[tokio::test]
async fn api_key_create_list_revoke_and_plaintext_once() {
    let state = setup(false, false, 20).await;
    let (owner_id, admin_key) = seed_owner(&state).await;
    let app = build_router(state.clone());

    // Create via HTTP
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/keys")
                .header("authorization", format!("Bearer {admin_key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "name": "ci-bot",
                        "scopes": ["papers:read", "keys:manage"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = json_body(resp).await;
    let plaintext = created["key"].as_str().unwrap().to_string();
    assert!(plaintext.starts_with("amk_"));
    let key_id = created["id"].as_str().unwrap().to_string();

    // DB must not store plaintext
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT convert_from(key_sha256, 'UTF8') FROM api_keys WHERE id = $1::uuid",
    )
    .bind(&key_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    // convert_from on binary hash is garbage / may error — assert hash path instead
    let hash: Vec<u8> = sqlx::query_scalar("SELECT key_sha256 FROM api_keys WHERE id = $1::uuid")
        .bind(&key_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(hash.len(), 32);
    assert_ne!(hash, plaintext.as_bytes());
    assert!(!String::from_utf8_lossy(&hash).contains("amk_"));
    let _ = stored;
    let _ = owner_id;

    // List never returns plaintext
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .header("authorization", format!("Bearer {admin_key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let page = json_body(resp).await;
    assert!(!page["data"].as_array().unwrap().is_empty());
    for item in page["data"].as_array().unwrap() {
        assert!(item.get("key").is_none());
        assert!(item.get("key_sha256").is_none());
    }

    // New key authenticates
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .header("authorization", format!("Bearer {plaintext}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Revoke
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/keys/{key_id}"))
                .header("authorization", format!("Bearer {admin_key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Revoked key → 401
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .header("authorization", format!("Bearer {plaintext}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(resp).await;
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");

    // Idempotent revoke
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/keys/{key_id}"))
                .header("authorization", format!("Bearer {admin_key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn expired_key_is_unauthorized() {
    let state = setup(false, false, 20).await;
    let scholar_id = sso::ensure_scholar(&state.db, "exp-owner", "Exp")
        .await
        .unwrap();
    // Insert a key that is already expired via SQL after create with future, then backdate.
    let created = keys::create_key(
        &state.db,
        scholar_id,
        CreateKeyRequest {
            name: "expiring".into(),
            scopes: vec![scope::KEYS_MANAGE.to_string()],
            expires_at: Some(Utc::now() + ChronoDuration::hours(1)),
        },
        &[scope::KEYS_MANAGE.to_string()],
        true,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE api_keys SET expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(created.id)
        .execute(&state.db)
        .await
        .unwrap();

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .header("authorization", format!("Bearer {}", created.key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_scope_is_forbidden() {
    let state = setup(false, false, 20).await;
    let scholar_id = sso::ensure_scholar(&state.db, "ro-owner", "RO")
        .await
        .unwrap();
    let created = keys::create_key(
        &state.db,
        scholar_id,
        CreateKeyRequest {
            name: "read-only".into(),
            scopes: vec![scope::PAPERS_READ.to_string()],
            expires_at: None,
        },
        &[scope::PAPERS_READ.to_string()],
        true,
    )
    .await
    .unwrap();

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .header("authorization", format!("Bearer {}", created.key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = json_body(resp).await;
    assert_eq!(body["error"]["code"], "FORBIDDEN");
}

#[tokio::test]
async fn unauthenticated_is_unauthorized() {
    let state = setup(false, false, 20).await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = json_body(resp).await;
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn session_cookie_auth_roundtrip() {
    let state = setup(false, false, 20).await;
    let sso = state.sso.as_ref().unwrap();
    let user = SsoUser {
        workspace_id: None,
        sub: "session-user-1".into(),
        login: Some("scholar1".into()),
        email: None,
        name: Some("Scholar One".into()),
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
    sso::upsert_scholar(&state.db, &user).await.unwrap();
    let token = sso.issue_session(&user).unwrap();

    let app = build_router(state);
    // /auth/me
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auth/me")
                .header("cookie", format!("automl_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let me = json_body(resp).await;
    assert_eq!(me["sub"], "session-user-1");

    // Use session for keys list
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/keys")
                .header("cookie", format!("automl_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Logout is POST (fleet w-id-axum convention)
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("cookie", format!("automl_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn rate_limit_returns_429_with_code_and_retry_after() {
    let state = setup(false, false, 1).await; // write burst 1
    let (_owner, key) = seed_owner(&state).await;
    let app = build_router(state);

    let mut saw_429 = false;
    for _ in 0..8 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("authorization", format!("Bearer {key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": format!("rl-{}", Uuid::new_v4()),
                            "scopes": ["papers:read"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        if resp.status() == StatusCode::TOO_MANY_REQUESTS {
            assert!(resp.headers().get("retry-after").is_some());
            let body = json_body(resp).await;
            assert_eq!(body["error"]["code"], "RATE_LIMITED");
            saw_429 = true;
            break;
        }
    }
    assert!(saw_429, "expected 429 under tight write burst");
}

#[tokio::test]
async fn openapi_has_security_schemes() {
    let state = setup(false, false, 20).await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let doc = json_body(resp).await;
    let schemes = &doc["components"]["securitySchemes"];
    assert!(schemes.get("bearer_api_key").is_some(), "{schemes}");
    assert!(schemes.get("session_cookie").is_some(), "{schemes}");
    // paths rewritten under /api/v1
    assert!(doc["paths"].get("/api/v1/keys").is_some());
    assert!(doc["paths"].get("/api/v1/health").is_some());
}

#[tokio::test]
async fn dev_admin_absent_when_not_enabled() {
    let state = setup(false, false, 20).await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/dev-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Route not registered → 404 (axum default) or method not allowed
    assert!(
        matches!(
            resp.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "status={}",
        resp.status()
    );
}

#[tokio::test]
async fn dev_admin_works_when_dual_gated() {
    let state = setup(true, false, 20).await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/dev-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("set-cookie").is_some());
    let body = json_body(resp).await;
    assert_eq!(body["w_id"], "dev-admin");
    assert!(body["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s == "admin"));
}

#[tokio::test]
async fn dev_admin_absent_in_production_even_if_flag_set() {
    let state = setup(true, true, 20).await;
    assert!(!state.cfg.dev_admin_enabled());
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/dev-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        matches!(
            resp.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ),
        "status={}",
        resp.status()
    );
}

// Silence unused import warning if ConnectInfo path changes.
#[allow(dead_code)]
fn _addr() -> SocketAddr {
    "127.0.0.1:1".parse().unwrap()
}

#[allow(dead_code)]
fn _duration() -> Duration {
    Duration::from_secs(1)
}
