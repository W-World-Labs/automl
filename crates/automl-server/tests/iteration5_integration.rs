//! Integration tests for iteration 5: compute profiles, templates + manifest
//! ETag, dedicated Gitea integration (repo provisioning + push webhook).
//! Requires DATABASE_URL (same as the other integration suites).

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use automl_core::scope;
use automl_server::{
    auth::{
        keys::{self, CreateKeyRequest},
        sso,
    },
    build_router,
    config::{Config, GiteaConfig, RateLimitConfig},
    domain::{
        gitea::{self, GiteaClient},
        papers,
    },
    state::AppState,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;
use w_id_sdk::SsoConfig;

const WEBHOOK_SECRET: &str = "test-webhook-secret";
const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

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
            .join(format!("automl-it5-objects-{}", Uuid::new_v4()))
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

fn gitea_client(base_url: &str) -> Arc<GiteaClient> {
    Arc::new(GiteaClient::new(GiteaConfig {
        url: base_url.to_string(),
        token: "test-token".into(),
        org: "papers".into(),
        webhook_secret: WEBHOOK_SECRET.into(),
    }))
}

async fn setup(gitea: Option<Arc<GiteaClient>>) -> AppState {
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
        gitea,
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
    keys::create_key(
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
    .unwrap()
    .key
}

fn req(method: &str, uri: &str, key: &str, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {key}"));
    if body.is_some() {
        b = b.header("content-type", "application/json");
    }
    b.body(match body {
        Some(v) => Body::from(v.to_string()),
        None => Body::empty(),
    })
    .unwrap()
}

/// Mock Gitea: counts repo-creation calls; `fail` forces 500, `conflict` 409.
struct MockGitea {
    calls: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    conflict: Arc<AtomicBool>,
    base_url: String,
}

async fn spawn_mock_gitea() -> MockGitea {
    let calls = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let conflict = Arc::new(AtomicBool::new(false));
    let (c2, f2, k2) = (calls.clone(), fail.clone(), conflict.clone());
    let app = Router::new().route(
        "/api/v1/orgs/{org}/repos",
        post(move || {
            let (c, f, k) = (c2.clone(), f2.clone(), k2.clone());
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                if f.load(Ordering::SeqCst) {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else if k.load(Ordering::SeqCst) {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::CREATED
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    MockGitea {
        calls,
        fail,
        conflict,
        base_url: format!("http://{addr}"),
    }
}

fn sign(body: &str) -> String {
    w_crypto::hmac_sha256_hex(WEBHOOK_SECRET.as_bytes(), body.as_bytes())
}

// ── compute profiles ─────────────────────────────────────────────────────────

#[tokio::test]
async fn compute_profile_idempotent_and_filterable() {
    let state = setup(None).await;
    let key = mint_key(
        &state,
        &format!("it5-compute-{}", Uuid::new_v4()),
        &[scope::REPRODUCE_SUBMIT, scope::PAPERS_READ],
    )
    .await;
    let app = build_router(state.clone());

    // Same content, different key order → same profile_hash, second call reuses.
    let p1 = json!({"profile": {"gpu": "H100", "os": {"name": "linux", "kernel": "6.8"}}});
    let p2 = json!({"profile": {"os": {"kernel": "6.8", "name": "linux"}, "gpu": "H100"}});

    let r1 = app
        .clone()
        .oneshot(req("POST", "/api/v1/compute/profiles", &key, Some(p1)))
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::CREATED);
    let v1 = json_body(r1).await;

    let r2 = app
        .clone()
        .oneshot(req("POST", "/api/v1/compute/profiles", &key, Some(p2)))
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::OK, "identical content reuses");
    let v2 = json_body(r2).await;
    assert_eq!(v1["id"], v2["id"]);
    assert_eq!(v1["profile_hash"], v2["profile_hash"]);

    // Non-object profile rejected.
    let bad = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/compute/profiles",
            &key,
            Some(json!({"profile": ["not", "an", "object"]})),
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    // Foreign agent_id rejected.
    let foreign = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/compute/profiles",
            &key,
            Some(json!({"agent_id": Uuid::new_v4(), "profile": {"gpu": "A100"}})),
        ))
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::BAD_REQUEST);

    // Own agent attribution + filter.
    let owner = sso::ensure_scholar(&state.db, "it5-compute-owner", "o")
        .await
        .unwrap();
    let key_o = mint_key(
        &state,
        "it5-compute-owner",
        &[scope::REPRODUCE_SUBMIT, scope::PAPERS_READ],
    )
    .await;
    let (agent_id,): (Uuid,) =
        sqlx::query_as("INSERT INTO agents (owner_id, name) VALUES ($1, 'bot') RETURNING id")
            .bind(owner)
            .fetch_one(&state.db)
            .await
            .unwrap();
    let r = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/compute/profiles",
            &key_o,
            Some(
                json!({"agent_id": agent_id, "profile": {"gpu": "B200", "nonce": Uuid::new_v4()}}),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);

    let list = app
        .clone()
        .oneshot(req(
            "GET",
            &format!("/api/v1/compute/profiles?agent_id={agent_id}"),
            &key_o,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let page = json_body(list).await;
    let data = page["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["agent_id"], json!(agent_id));
}

// ── templates + manifest ETag ────────────────────────────────────────────────

#[tokio::test]
async fn template_manifest_etag_304() {
    let state = setup(None).await;
    let admin_key = mint_key(
        &state,
        &format!("it5-admin-{}", Uuid::new_v4()),
        &[scope::ADMIN],
    )
    .await;
    let app = build_router(state.clone());

    let r = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/templates",
            &admin_key,
            Some(json!({
                "name": "repro-basic",
                "kind": "reproduce",
                "body": "steps:\n  - run: make reproduce\n",
                "manifest": {"skills": ["python", "cuda"]},
            })),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let tpl = json_body(r).await;
    let id = tpl["id"].as_str().unwrap().to_string();
    assert_eq!(tpl["version"], 1);

    // First manifest fetch: 200 + ETag.
    let m1 = app
        .clone()
        .oneshot(req(
            "GET",
            &format!("/api/v1/templates/{id}/manifest"),
            &admin_key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(m1.status(), StatusCode::OK);
    let etag = m1
        .headers()
        .get(http::header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Conditional refetch: 304.
    let m2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/templates/{id}/manifest"))
                .header("authorization", format!("Bearer {admin_key}"))
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(m2.status(), StatusCode::NOT_MODIFIED);

    // Patch bumps version → ETag changes → conditional fetch misses.
    let p = app
        .clone()
        .oneshot(req(
            "PATCH",
            &format!("/api/v1/templates/{id}"),
            &admin_key,
            Some(json!({"manifest": {"skills": ["python", "cuda", "docker"]}})),
        ))
        .await
        .unwrap();
    assert_eq!(p.status(), StatusCode::OK);
    assert_eq!(json_body(p).await["version"], 2);

    let m3 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/templates/{id}/manifest"))
                .header("authorization", format!("Bearer {admin_key}"))
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(m3.status(), StatusCode::OK);
    assert_ne!(
        m3.headers()
            .get(http::header::ETAG)
            .unwrap()
            .to_str()
            .unwrap(),
        etag
    );

    // Non-admin cannot write templates.
    let reader_key = mint_key(
        &state,
        &format!("it5-reader-{}", Uuid::new_v4()),
        &[scope::PAPERS_READ],
    )
    .await;
    let f = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/templates",
            &reader_key,
            Some(json!({"name": "x", "kind": "paper"})),
        ))
        .await
        .unwrap();
    assert_eq!(f.status(), StatusCode::FORBIDDEN);
}

// ── gitea webhook ────────────────────────────────────────────────────────────

#[tokio::test]
async fn webhook_signature_gate_and_push_sync() {
    let mock = spawn_mock_gitea().await;
    let state = setup(Some(gitea_client(&mock.base_url))).await;
    let key = mint_key(
        &state,
        &format!("it5-hook-{}", Uuid::new_v4()),
        &[scope::PAPERS_READ, scope::PAPERS_WRITE],
    )
    .await;
    let app = build_router(state.clone());

    // Platform-hosted paper — the only kind the push webhook may touch
    // (state machine: `unknown` external pointers are webhook-unreachable).
    let r = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/papers",
            &key,
            Some(json!({"title": "hook target"})),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let paper = json_body(r).await;
    let paper_id = paper["id"].as_str().unwrap().to_string();
    assert_eq!(paper["code_status"], "repo_pending");

    // Author pins the commit used for reproduction; the webhook only ever
    // moves latest_commit, never the pin.
    let pin = app
        .clone()
        .oneshot(req(
            "PATCH",
            &format!("/api/v1/papers/{paper_id}"),
            &key,
            Some(json!({"commit_sha": COMMIT})),
        ))
        .await
        .unwrap();
    assert_eq!(pin.status(), StatusCode::OK);

    // External-pointer papers must be untouchable by the webhook.
    let ext = json_body(
        app.clone()
            .oneshot(req(
                "POST",
                "/api/v1/papers",
                &key,
                Some(json!({
                    "title": "external pointer",
                    "repo_url": "https://example.test/x.git",
                    "commit_sha": COMMIT,
                })),
            ))
            .await
            .unwrap(),
    )
    .await;
    let ext_id = ext["id"].as_str().unwrap().to_string();

    let new_commit = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let payload = json!({
        "repository": {"name": paper_id},
        "after": new_commit,
    })
    .to_string();

    // Bad signature → 401, nothing applied.
    let bad = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/gitea/events")
                .header("x-gitea-event", "push")
                .header("x-gitea-signature", sign("different-body"))
                .header("content-type", "application/json")
                .body(Body::from(payload.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

    // Missing signature → 401.
    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/gitea/events")
                .header("x-gitea-event", "push")
                .header("content-type", "application/json")
                .body(Body::from(payload.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    // Good signature + push → 204, paper syncs.
    let good = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/gitea/events")
                .header("x-gitea-event", "push")
                .header("x-gitea-signature", sign(&payload))
                .header("content-type", "application/json")
                .body(Body::from(payload.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(good.status(), StatusCode::NO_CONTENT);

    let p = json_body(
        app.clone()
            .oneshot(req(
                "GET",
                &format!("/api/v1/papers/{paper_id}"),
                &key,
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(p["latest_commit"], new_commit);
    assert_eq!(p["code_status"], "ready");
    // Pinned commit untouched by webhook sync.
    assert_eq!(p["commit_sha"], COMMIT);

    // A push naming an external-pointer paper is acknowledged but must not
    // mutate it (state machine: `unknown` is terminal).
    let ext_payload = json!({
        "repository": {"name": ext_id},
        "after": new_commit,
    })
    .to_string();
    let ext_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/gitea/events")
                .header("x-gitea-event", "push")
                .header("x-gitea-signature", sign(&ext_payload))
                .header("content-type", "application/json")
                .body(Body::from(ext_payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ext_resp.status(), StatusCode::NO_CONTENT);
    let ext_after = json_body(
        app.clone()
            .oneshot(req("GET", &format!("/api/v1/papers/{ext_id}"), &key, None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(ext_after["code_status"], "unknown");
    assert!(ext_after["latest_commit"].is_null());

    // Unknown event type → 204 silently ignored.
    let other = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/gitea/events")
                .header("x-gitea-event", "issues")
                .header("x-gitea-signature", sign(&payload))
                .header("content-type", "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn webhook_501_when_unconfigured() {
    let state = setup(None).await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/gitea/events")
                .header("x-gitea-event", "push")
                .header("x-gitea-signature", "00")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    let v = json_body(resp).await;
    assert_eq!(v["error"]["code"], "NOT_IMPLEMENTED");
}

// ── repo provisioning ────────────────────────────────────────────────────────

#[tokio::test]
async fn platform_hosted_paper_provisions_with_compensation() {
    let mock = spawn_mock_gitea().await;
    let client = gitea_client(&mock.base_url);
    let state = setup(Some(client.clone())).await;
    let owner = sso::ensure_scholar(&state.db, &format!("it5-prov-{}", Uuid::new_v4()), "p")
        .await
        .unwrap();

    // Gitea down → paper stays repo_pending after a failed provisioning pass.
    mock.fail.store(true, Ordering::SeqCst);
    let paper = papers::create(
        &state.db,
        owner,
        serde_json::from_value(json!({"title": "platform hosted"})).unwrap(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(paper.code_status, "repo_pending");
    assert_eq!(paper.repo_url, "");
    assert_eq!(paper.commit_sha, "");

    gitea::provision_pending(&state.db, &client).await.unwrap();
    let p = papers::get(&state.db, paper.id).await.unwrap();
    assert_eq!(p.code_status, "repo_pending", "failure keeps pending");

    // Gitea back up → compensation sweep provisions and fills repo_url.
    mock.fail.store(false, Ordering::SeqCst);
    gitea::provision_pending(&state.db, &client).await.unwrap();
    let p = papers::get(&state.db, paper.id).await.unwrap();
    assert_eq!(p.code_status, "repo_ready");
    assert_eq!(
        p.repo_url,
        format!("{}/papers/{}.git", mock.base_url, paper.id)
    );

    // Idempotency: 409 from Gitea (repo exists) also counts as success.
    mock.conflict.store(true, Ordering::SeqCst);
    let paper2 = papers::create(
        &state.db,
        owner,
        serde_json::from_value(json!({"title": "already exists"})).unwrap(),
        true,
    )
    .await
    .unwrap();
    gitea::provision_paper(&state.db, &client, paper2.id)
        .await
        .unwrap();
    let p2 = papers::get(&state.db, paper2.id).await.unwrap();
    assert_eq!(p2.code_status, "repo_ready");
    assert!(mock.calls.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn paper_without_repo_url_requires_platform_hosting() {
    let state = setup(None).await;
    let key = mint_key(
        &state,
        &format!("it5-nrepo-{}", Uuid::new_v4()),
        &[scope::PAPERS_WRITE],
    )
    .await;
    let app = build_router(state.clone());

    // Integration disabled → repo_url mandatory.
    let r = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/papers",
            &key,
            Some(json!({"title": "no repo"})),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    // External pointer without a pinned commit → 400.
    let r = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/papers",
            &key,
            Some(json!({"title": "no pin", "repo_url": "https://example.test/y.git"})),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unpinned_platform_paper_refuses_runs() {
    let state = setup(None).await;
    let owner = sso::ensure_scholar(&state.db, &format!("it5-run-{}", Uuid::new_v4()), "r")
        .await
        .unwrap();
    let paper = papers::create(
        &state.db,
        owner,
        serde_json::from_value(json!({"title": "unpinned"})).unwrap(),
        true,
    )
    .await
    .unwrap();

    let err = automl_server::domain::reproduce::register_run(
        &state.db,
        "scholar:test",
        serde_json::from_value(json!({
            "paper_id": paper.id,
            "content_hash": "c".repeat(64),
        }))
        .unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, automl_core::ErrorCode::Conflict);
}
