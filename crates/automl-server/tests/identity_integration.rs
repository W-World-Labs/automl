//! Integration tests for iteration 4 (identity + signature verification).

use std::sync::Arc;

use automl_core::scope;
use automl_server::{
    auth::{
        keys::{self, CreateKeyRequest},
        sso,
    },
    build_router,
    config::{Config, RateLimitConfig},
    domain::agents::canonical_evidence_message,
    state::AppState,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use rand::rngs::OsRng;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;
use w_id_sdk::SsoConfig;

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
            .join(format!("automl-id-objects-{}", Uuid::new_v4()))
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

async fn mint_key(state: &AppState, w_id: &str) -> (Uuid, String) {
    let scholar = sso::ensure_scholar(&state.db, w_id, w_id).await.unwrap();
    let scopes = [
        scope::PAPERS_READ,
        scope::PAPERS_WRITE,
        scope::REPRODUCE_SUBMIT,
    ];
    let created = keys::create_key(
        &state.db,
        scholar,
        CreateKeyRequest {
            name: format!("k-{w_id}"),
            scopes: scopes.iter().map(|s| (*s).to_string()).collect(),
            expires_at: None,
        },
        &scopes.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        true,
    )
    .await
    .unwrap();
    (scholar, created.key)
}

#[tokio::test]
async fn scholar_profile_list_get_patch() {
    let state = setup().await;
    let (sid, key) = mint_key(&state, &format!("sch-{}", Uuid::new_v4())).await;
    let app = build_router(state);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/scholars/{sid}"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/scholars/{sid}"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "display_name": "Ada Lovelace",
                        "affiliation": "Analytical Engine Lab",
                        "orcid": "0000-0002-1825-0097",
                        "bio": "first programmer"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["display_name"], "Ada Lovelace");
    assert_eq!(body["orcid"], "0000-0002-1825-0097");

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/scholars?limit=5")
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!json_body(resp).await["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn agent_device_register_and_revoke() {
    let state = setup().await;
    let (_sid, key) = mint_key(&state, &format!("own-{}", Uuid::new_v4())).await;
    let app = build_router(state);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/agents")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "repro-bot"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let agent_id = json_body(resp).await["id"].as_str().unwrap().to_string();

    let sk = SigningKey::generate(&mut OsRng);
    let pk_hex: String = sk
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/agents/{agent_id}/devices"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"pubkey": pk_hex, "env_fingerprint": {"gpu": "none"}}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let device_id = json_body(resp).await["id"].as_str().unwrap().to_string();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/agents/{agent_id}/devices/{device_id}"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Idempotent
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/agents/{agent_id}/devices/{device_id}"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn bad_signature_and_revoked_device_rejected() {
    let state = setup().await;
    let (_sid, key) = mint_key(&state, &format!("sig-{}", Uuid::new_v4())).await;
    let app = build_router(state);

    // Agent + device
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/agents")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"name": "sig-bot"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let agent_id = json_body(resp).await["id"].as_str().unwrap().to_string();

    let sk = SigningKey::generate(&mut OsRng);
    let pk_hex: String = sk
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/agents/{agent_id}/devices"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"pubkey": pk_hex, "env_fingerprint": {}}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let device_id = json_body(resp).await["id"].as_str().unwrap().to_string();

    // Create a paper + run
    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/papers")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Sig Paper",
                        "repo_url": "https://example.test/r.git",
                        "commit_sha": COMMIT,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let paper_id = json_body(resp).await["id"].as_str().unwrap().to_string();
    let content_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/reproduce/runs")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"paper_id": paper_id, "content_hash": content_hash}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let run_id = json_body(resp).await["id"].as_str().unwrap().to_string();
    let run_uuid = Uuid::parse_str(&run_id).unwrap();

    // Bad signature (wrong message)
    let bad_sig = sk.sign(b"not the evidence");
    let bad_hex: String = bad_sig
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/reproduce/runs/{run_id}/evidence"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "exit_code": 0,
                        "metrics": {},
                        "artifact_hashes": {},
                        "env_fingerprint": {},
                        "signature_hex": bad_hex,
                        "signer_key_id": device_id,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(resp).await["error"]["code"], "SIGNATURE_INVALID");

    // Revoke device then good signature should still fail
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/agents/{agent_id}/devices/{device_id}"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // New run (previous still registered)
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/reproduce/runs")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"paper_id": paper_id, "content_hash": content_hash}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let run_id2 = json_body(resp).await["id"].as_str().unwrap().to_string();
    let run_uuid2 = Uuid::parse_str(&run_id2).unwrap();
    let metrics = json!({});
    let env = json!({});
    let artifacts = json!({});
    let msg = canonical_evidence_message(run_uuid2, 0, &None, &artifacts, &metrics, &env);
    let sig = sk.sign(&msg);
    let sig_hex: String = sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect();

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/reproduce/runs/{run_id2}/evidence"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "exit_code": 0,
                        "metrics": metrics,
                        "artifact_hashes": artifacts,
                        "env_fingerprint": env,
                        "signature_hex": sig_hex,
                        "signer_key_id": device_id,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(resp).await["error"]["code"], "SIGNATURE_INVALID");
    let _ = run_uuid; // silence if unused
}
