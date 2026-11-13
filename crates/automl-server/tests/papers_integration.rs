//! Integration tests for iteration 3 (papers core).
//! Requires DATABASE_URL (same as auth_integration).

use std::sync::Arc;

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
use bytes::Bytes;
use http_body_util::BodyExt;
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
            .join(format!("automl-paper-objects-{}", Uuid::new_v4()))
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

async fn mint_key(state: &AppState, w_id: &str, scopes: &[&str]) -> String {
    let scholar = sso::ensure_scholar(&state.db, w_id, w_id).await.unwrap();
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
    created.key
}

const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

async fn post_paper(app: axum::Router, key: &str, title: &str) -> Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/papers")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": title,
                        "abstract": "a",
                        "authors": ["Ada"],
                        "repo_url": "https://example.test/repo.git",
                        "commit_sha": COMMIT,
                        "reproduce_path": "reproduce.yaml"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    json_body(resp).await
}

#[tokio::test]
async fn papers_crud_and_illegal_status_transition() {
    let state = setup().await;
    let key = mint_key(
        &state,
        &format!("author-{}", Uuid::new_v4()),
        &[scope::PAPERS_READ, scope::PAPERS_WRITE],
    )
    .await;
    let admin_key = mint_key(
        &state,
        &format!("gatekeeper-{}", Uuid::new_v4()),
        &[scope::ADMIN],
    )
    .await;
    let app = build_router(state);

    let paper = post_paper(app.clone(), &key, "Genesis").await;
    let id = paper["id"].as_str().unwrap();
    assert_eq!(paper["review_status"], "draft");

    let patch_status = |key: String, status: &'static str| {
        let app = app.clone();
        let uri = format!("/api/v1/papers/{id}");
        async move {
            app.oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(uri)
                    .header("authorization", format!("Bearer {key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "review_status": status }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    // illegal: draft → accepted (even for admin — state machine first)
    let resp = patch_status(admin_key.clone(), "accepted").await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = json_body(resp).await;
    assert_eq!(body["error"]["code"], "CONFLICT");

    // author submits…
    let resp = patch_status(key.clone(), "submitted").await;
    assert_eq!(resp.status(), StatusCode::OK);

    // …but cannot advance into review or decide (human gate is admin-only)
    for status in ["under_review", "accepted"] {
        let resp = patch_status(key.clone(), status).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "author → {status}");
    }

    // admin advances and decides
    for status in ["under_review", "accepted"] {
        let resp = patch_status(admin_key.clone(), status).await;
        assert_eq!(resp.status(), StatusCode::OK, "admin → {status}");
        let body = json_body(resp).await;
        assert_eq!(body["review_status"], status);
    }
}

#[tokio::test]
async fn verified_by_n_requires_valid_signatures() {
    // Iteration 4: unsigned accepted runs do not count toward verified_by_n.
    // Two distinct signed runners with matching metrics → verified_by_2.
    use automl_server::domain::agents::canonical_evidence_message;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use uuid::Uuid as U;

    let state = setup().await;
    let scopes = [
        scope::PAPERS_READ,
        scope::PAPERS_WRITE,
        scope::REPRODUCE_SUBMIT,
    ];
    let key_a = mint_key(&state, &format!("runner-a-{}", Uuid::new_v4()), &scopes).await;
    let key_b = mint_key(&state, &format!("runner-b-{}", Uuid::new_v4()), &scopes).await;
    let app = build_router(state.clone());

    let paper = post_paper(app.clone(), &key_a, "Repro Paper").await;
    let paper_id = paper["id"].as_str().unwrap().to_string();
    let content_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    async fn make_agent_device(
        app: axum::Router,
        key: &str,
        name: &str,
    ) -> (String, SigningKey, String) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("authorization", format!("Bearer {key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": name}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let agent = json_body(resp).await;
        let agent_id = agent["id"].as_str().unwrap().to_string();

        let sk = SigningKey::generate(&mut OsRng);
        let pk_hex: String = sk
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{agent_id}/devices"))
                    .header("authorization", format!("Bearer {key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "pubkey": pk_hex,
                            "env_fingerprint": {"os": "linux"}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let device = json_body(resp).await;
        (agent_id, sk, device["id"].as_str().unwrap().to_string())
    }

    let (_a_id, sk_a, dev_a) = make_agent_device(app.clone(), &key_a, "agent-a").await;
    let (_b_id, sk_b, dev_b) = make_agent_device(app.clone(), &key_b, "agent-b").await;

    async fn register_and_evidence(
        app: axum::Router,
        key: &str,
        paper_id: &str,
        content_hash: &str,
        sk: Option<&SigningKey>,
        device_id: Option<&str>,
    ) {
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
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let run = json_body(resp).await;
        let run_id = run["id"].as_str().unwrap().to_string();
        let run_uuid = U::parse_str(&run_id).unwrap();

        let metrics = json!({"accuracy": 0.91});
        let env = json!({"os": "linux"});
        let artifacts = json!({});
        let mut body = json!({
            "exit_code": 0,
            "metrics": metrics,
            "artifact_hashes": artifacts,
            "env_fingerprint": env
        });
        if let (Some(sk), Some(did)) = (sk, device_id) {
            let msg = canonical_evidence_message(run_uuid, 0, &None, &artifacts, &metrics, &env);
            let sig = sk.sign(&msg);
            let sig_hex: String = sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
            body["signature_hex"] = json!(sig_hex);
            body["signer_key_id"] = json!(did);
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/reproduce/runs/{run_id}/evidence"))
                    .header("authorization", format!("Bearer {key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["run"]["status"], "accepted");
    }

    // Unsigned — accepted but not verified
    register_and_evidence(app.clone(), &key_a, &paper_id, content_hash, None, None).await;
    // Signed by A (twice — same runner)
    register_and_evidence(
        app.clone(),
        &key_a,
        &paper_id,
        content_hash,
        Some(&sk_a),
        Some(&dev_a),
    )
    .await;
    register_and_evidence(
        app.clone(),
        &key_a,
        &paper_id,
        content_hash,
        Some(&sk_a),
        Some(&dev_a),
    )
    .await;
    // Signed by B
    register_and_evidence(
        app.clone(),
        &key_b,
        &paper_id,
        content_hash,
        Some(&sk_b),
        Some(&dev_b),
    )
    .await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/papers/{paper_id}/reproduction"))
                .header("authorization", format!("Bearer {key_a}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let summary = json_body(resp).await;
    assert_eq!(summary["verified_by_n"], 2);
    assert!(summary["runs_total"].as_u64().unwrap() >= 4);
    assert_eq!(summary["consensus_metrics"]["accuracy"], 0.91);
}

#[tokio::test]
async fn asset_presign_put_commit_and_sha_mismatch() {
    let state = setup().await;
    let key = mint_key(
        &state,
        &format!("asset-{}", Uuid::new_v4()),
        &[scope::PAPERS_READ, scope::PAPERS_WRITE, scope::ASSETS_WRITE],
    )
    .await;
    let payload = b"hello-asset-bytes";
    let sha = w_crypto::sha256_hex(payload);

    let storage = state.storage.clone();
    let app = build_router(state);

    let paper = post_paper(app.clone(), &key, "Asset Paper").await;
    let paper_id = paper["id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/papers/{paper_id}/assets"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "filename": "log.txt",
                        "sha256": sha,
                        "size_bytes": payload.len(),
                        "content_type": "text/plain"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = json_body(resp).await;
    let asset_id = created["asset"]["id"].as_str().unwrap().to_string();
    let object_key = created["asset"]["object_key"].as_str().unwrap().to_string();
    assert!(created["upload_url"]
        .as_str()
        .unwrap()
        .contains(&object_key));

    // Simulate client direct upload (local backend).
    storage
        .put(&object_key, Bytes::from_static(payload), Some("text/plain"))
        .await
        .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/assets/{asset_id}/commit"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["status"], "committed");

    // Mismatch path: new asset, upload wrong bytes
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/papers/{paper_id}/assets"))
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "filename": "bad.txt",
                        "sha256": sha,
                        "size_bytes": payload.len(),
                        "content_type": "text/plain"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bad = json_body(resp).await;
    let bad_id = bad["asset"]["id"].as_str().unwrap();
    let bad_key = bad["asset"]["object_key"].as_str().unwrap();
    storage
        .put(bad_key, Bytes::from_static(b"WRONG"), Some("text/plain"))
        .await
        .unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/assets/{bad_id}/commit"))
                .header("authorization", format!("Bearer {key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(resp).await["error"]["code"], "CONFLICT");
}
