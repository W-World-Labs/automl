//! Integration tests for iteration 6: forum soft delete, anonymous feedback
//! with the strictest rate bucket, and the outbound webhook pipeline
//! (transactional outbox → fan-out → signed delivery → backoff → dead letter).
//! Requires DATABASE_URL.

use std::sync::{
    atomic::{AtomicI32, AtomicUsize, Ordering},
    Arc, Mutex,
};

use automl_core::scope;
use automl_server::{
    auth::{
        keys::{self, CreateKeyRequest},
        sso,
    },
    build_router,
    config::{Config, RateLimitConfig},
    domain::{events, papers},
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

const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_config(feedback_burst: u32) -> Config {
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
            feedback_rps: 1,
            feedback_burst,
        },
        storage_backend: "local".into(),
        storage_local_root: std::env::temp_dir()
            .join(format!("automl-it6-objects-{}", Uuid::new_v4()))
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

async fn setup(feedback_burst: u32) -> AppState {
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
    let cfg = test_config(feedback_burst);
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

/// Local webhook receiver: records signatures+bodies, programmable status.
struct Receiver {
    url: String,
    hits: Arc<AtomicUsize>,
    respond_with: Arc<AtomicI32>,
    seen: Arc<Mutex<Vec<(String, String)>>>, // (signature, body)
}

async fn spawn_receiver() -> Receiver {
    let hits = Arc::new(AtomicUsize::new(0));
    let respond_with = Arc::new(AtomicI32::new(200));
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let (h2, r2, s2) = (hits.clone(), respond_with.clone(), seen.clone());
    let app = Router::new().route(
        "/hook",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let (h, r, s) = (h2.clone(), r2.clone(), s2.clone());
            async move {
                h.fetch_add(1, Ordering::SeqCst);
                let sig = headers
                    .get("x-automl-signature")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                s.lock().unwrap().push((sig, body));
                StatusCode::from_u16(r.load(Ordering::SeqCst) as u16).unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Receiver {
        url: format!("http://{addr}/hook"),
        hits,
        respond_with,
        seen,
    }
}

fn http_client() -> reqwest::Client {
    w_http::client_with_timeout(std::time::Duration::from_secs(5))
}

/// The delivery pipeline reads shared tables (subscriptions / outbox /
/// deliveries), so pipeline tests serialize on this lock and quiesce leftovers
/// from earlier tests/runs — otherwise fan-out crosses test boundaries.
static WEBHOOK_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

fn webhook_lock() -> &'static tokio::sync::Mutex<()> {
    WEBHOOK_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn quiesce_pipeline(db: &sqlx::PgPool) {
    sqlx::query("UPDATE webhook_subscriptions SET active = false")
        .execute(db)
        .await
        .unwrap();
    sqlx::query("UPDATE webhook_deliveries SET status = 'dead' WHERE status = 'pending'")
        .execute(db)
        .await
        .unwrap();
    sqlx::query("UPDATE outbox_events SET processed_at = now() WHERE processed_at IS NULL")
        .execute(db)
        .await
        .unwrap();
}

/// Force every pending delivery due now (tests never wait for wall-clock backoff).
async fn force_due(db: &sqlx::PgPool) {
    sqlx::query("UPDATE webhook_deliveries SET next_retry_at = now() WHERE status = 'pending'")
        .execute(db)
        .await
        .unwrap();
}

// ── forum ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn forum_thread_post_lifecycle_and_soft_delete() {
    let state = setup(10_000).await;
    let author_wid = format!("it6-author-{}", Uuid::new_v4());
    let author_key = mint_key(
        &state,
        &author_wid,
        &[scope::FORUM_WRITE, scope::PAPERS_READ],
    )
    .await;
    let other_key = mint_key(
        &state,
        &format!("it6-other-{}", Uuid::new_v4()),
        &[scope::FORUM_WRITE, scope::PAPERS_READ],
    )
    .await;
    let admin_key = mint_key(
        &state,
        &format!("it6-admin-{}", Uuid::new_v4()),
        &[scope::ADMIN],
    )
    .await;
    let app = build_router(state.clone());

    // Seeded categories exist.
    let cats = json_body(
        app.clone()
            .oneshot(req("GET", "/api/v1/forum/categories", &author_key, None))
            .await
            .unwrap(),
    )
    .await;
    let cat_id = cats["data"][0]["id"].as_str().unwrap().to_string();
    assert!(cats["data"].as_array().unwrap().len() >= 4);

    // Thread with first post.
    let r = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/forum/threads",
            &author_key,
            Some(json!({"category_id": cat_id, "title": "Repro tips", "body": "**markdown** stays raw"})),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let thread = json_body(r).await;
    let thread_id = thread["id"].as_str().unwrap().to_string();

    // Reply + pagination (limit=1 → cursor).
    let r = app
        .clone()
        .oneshot(req(
            "POST",
            &format!("/api/v1/forum/threads/{thread_id}/posts"),
            &other_key,
            Some(json!({"body": "reply"})),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let reply = json_body(r).await;
    let reply_id = reply["id"].as_str().unwrap().to_string();

    let page1 = json_body(
        app.clone()
            .oneshot(req(
                "GET",
                &format!("/api/v1/forum/threads/{thread_id}/posts?limit=1"),
                &author_key,
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(page1["data"].as_array().unwrap().len(), 1);
    let cursor = page1["next_cursor"].as_str().unwrap().to_string();
    assert_eq!(page1["data"][0]["body"], "**markdown** stays raw");
    let page2 = json_body(
        app.clone()
            .oneshot(req(
                "GET",
                &format!(
                    "/api/v1/forum/threads/{thread_id}/posts?limit=1&cursor={}",
                    urlencode(&cursor)
                ),
                &author_key,
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(page2["data"][0]["body"], "reply");

    // Non-author cannot delete someone else's post…
    let f = app
        .clone()
        .oneshot(req(
            "DELETE",
            &format!("/api/v1/forum/posts/{reply_id}"),
            &author_key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(f.status(), StatusCode::FORBIDDEN);

    // …but admin can (soft delete → gone from list).
    let d = app
        .clone()
        .oneshot(req(
            "DELETE",
            &format!("/api/v1/forum/posts/{reply_id}"),
            &admin_key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(d.status(), StatusCode::NO_CONTENT);
    let after = json_body(
        app.clone()
            .oneshot(req(
                "GET",
                &format!("/api/v1/forum/threads/{thread_id}/posts"),
                &author_key,
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(after["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["id"] != json!(reply_id)));

    // Author soft-deletes the thread → list invisible, direct fetch 404.
    let d = app
        .clone()
        .oneshot(req(
            "DELETE",
            &format!("/api/v1/forum/threads/{thread_id}"),
            &author_key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(d.status(), StatusCode::NO_CONTENT);
    let got = app
        .clone()
        .oneshot(req(
            "GET",
            &format!("/api/v1/forum/threads/{thread_id}"),
            &author_key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(got.status(), StatusCode::NOT_FOUND);

    // Notifications stub honors stub-is-501.
    let n = app
        .clone()
        .oneshot(req("GET", "/api/v1/notifications", &author_key, None))
        .await
        .unwrap();
    assert_eq!(n.status(), StatusCode::NOT_IMPLEMENTED);
}

fn urlencode(s: &str) -> String {
    s.replace('+', "%2B")
        .replace('|', "%7C")
        .replace(':', "%3A")
}

// ── feedback ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn feedback_anonymous_with_strict_rate_limit() {
    let state = setup(2).await; // burst of 2 on the feedback bucket
    let admin_key = mint_key(
        &state,
        &format!("it6-fb-admin-{}", Uuid::new_v4()),
        &[scope::ADMIN],
    )
    .await;
    let app = build_router(state.clone());

    // Anonymous submission — no Authorization header at all.
    let anon = |body: Value| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/feedback")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };

    let r = app
        .clone()
        .oneshot(anon(
            json!({"category": "bug", "body": "CLI panics on emoji"}),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let fb = json_body(r).await;
    assert_eq!(fb["status"], "new");
    let fb_id = fb["id"].as_str().unwrap().to_string();

    let r2 = app
        .clone()
        .oneshot(anon(json!({"category": "feature", "body": "dark mode"})))
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::CREATED);

    // Third within the burst window trips the strict bucket.
    let r3 = app
        .clone()
        .oneshot(anon(json!({"category": "other", "body": "spam"})))
        .await
        .unwrap();
    assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);

    // Unknown category rejected.
    let bad = app
        .clone()
        .oneshot(anon(json!({"category": "rant", "body": "x"})))
        .await
        .unwrap();
    // Rate bucket may fire first; accept either but never 201.
    assert_ne!(bad.status(), StatusCode::CREATED);

    // Admin triage: list + status flip; non-admin forbidden.
    let list = json_body(
        app.clone()
            .oneshot(req(
                "GET",
                "/api/v1/admin/feedback?status=new",
                &admin_key,
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == json!(fb_id)));

    let p = app
        .clone()
        .oneshot(req(
            "PATCH",
            &format!("/api/v1/admin/feedback/{fb_id}"),
            &admin_key,
            Some(json!({"status": "triaged"})),
        ))
        .await
        .unwrap();
    assert_eq!(p.status(), StatusCode::OK);
    assert_eq!(json_body(p).await["status"], "triaged");

    let non_admin_key = mint_key(
        &state,
        &format!("it6-fb-user-{}", Uuid::new_v4()),
        &[scope::PAPERS_READ],
    )
    .await;
    let f = app
        .clone()
        .oneshot(req("GET", "/api/v1/admin/feedback", &non_admin_key, None))
        .await
        .unwrap();
    assert_eq!(f.status(), StatusCode::FORBIDDEN);
}

// ── outbound webhooks ────────────────────────────────────────────────────────

#[tokio::test]
async fn paper_decision_delivers_signed_webhook() {
    let state = setup(10_000).await;
    let _guard = webhook_lock().lock().await;
    quiesce_pipeline(&state.db).await;
    let receiver = spawn_receiver().await;
    let owner = sso::ensure_scholar(&state.db, &format!("it6-wh-{}", Uuid::new_v4()), "w")
        .await
        .unwrap();
    let secret = "receiver-secret-0123456789";

    events::create_subscription(
        &state.db,
        owner,
        serde_json::from_value(json!({
            "url": receiver.url,
            "secret": secret,
            "events": ["paper.decided"],
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    // Drive a paper to a decision.
    let paper = papers::create(
        &state.db,
        owner,
        serde_json::from_value(json!({
            "title": "webhook paper",
            "repo_url": "https://example.test/w.git",
            "commit_sha": COMMIT,
        }))
        .unwrap(),
        false,
    )
    .await
    .unwrap();
    for status in ["submitted", "under_review", "accepted"] {
        papers::patch(
            &state.db,
            paper.id,
            serde_json::from_value(json!({"review_status": status})).unwrap(),
        )
        .await
        .unwrap();
    }

    // Exactly one outbox row for the decision.
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'paper.decided' AND payload->>'paper_id' = $1",
    )
    .bind(paper.id.to_string())
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(n, 1);

    // Worker pass delivers; receiver can verify the signature.
    events::run_once(&state.db, &http_client()).await.unwrap();
    assert_eq!(receiver.hits.load(Ordering::SeqCst), 1);
    let (sig, body) = receiver.seen.lock().unwrap()[0].clone();
    let expected = format!(
        "sha256={}",
        w_crypto::hmac_sha256_hex(secret.as_bytes(), body.as_bytes())
    );
    assert_eq!(sig, expected, "receiver-side signature verification");
    let payload: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(payload["event_type"], "paper.decided");
    assert_eq!(payload["data"]["decision"], "accepted");
    assert!(payload["event_id"].as_str().is_some());

    // Illegal transition rolls back → no new event (outbox is transactional).
    let err = papers::patch(
        &state.db,
        paper.id,
        serde_json::from_value(json!({"review_status": "rejected"})).unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, automl_core::ErrorCode::Conflict);
    let (n2,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM outbox_events WHERE event_type = 'paper.decided' AND payload->>'paper_id' = $1",
    )
    .bind(paper.id.to_string())
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(n2, 1, "rolled-back decision must not emit");
}

#[tokio::test]
async fn webhook_retry_then_recover_and_dead_letter() {
    let state = setup(10_000).await;
    let _guard = webhook_lock().lock().await;
    quiesce_pipeline(&state.db).await;
    let owner = sso::ensure_scholar(&state.db, &format!("it6-dl-{}", Uuid::new_v4()), "d")
        .await
        .unwrap();
    let http = http_client();

    // Receiver A: fails once, then recovers.
    let flaky = spawn_receiver().await;
    flaky.respond_with.store(500, Ordering::SeqCst);
    let sub = events::create_subscription(
        &state.db,
        owner,
        serde_json::from_value(json!({
            "url": flaky.url,
            "secret": "flaky-secret-0123456789",
            "events": ["repro.completed"],
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    // Emit an event directly through the outbox (unit of this test is delivery).
    let mut tx = state.db.begin().await.unwrap();
    events::emit(
        &mut tx,
        "repro.completed",
        json!({"run_id": Uuid::new_v4(), "status": "accepted"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    events::run_once(&state.db, &http).await.unwrap(); // attempt 1 → 500
    let (status, attempt): (String, i32) =
        sqlx::query_as("SELECT status, attempt FROM webhook_deliveries WHERE subscription_id = $1")
            .bind(sub.id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(attempt, 1);

    // Not due yet → a worker pass does nothing (backoff respected).
    events::run_once(&state.db, &http).await.unwrap();
    assert_eq!(flaky.hits.load(Ordering::SeqCst), 1);

    // Recover: force due, receiver healthy → delivered.
    flaky.respond_with.store(200, Ordering::SeqCst);
    force_due(&state.db).await;
    events::run_once(&state.db, &http).await.unwrap();
    let (status,): (String,) =
        sqlx::query_as("SELECT status FROM webhook_deliveries WHERE subscription_id = $1")
            .bind(sub.id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(status, "delivered");

    // Receiver B: always 500 → exhausts the schedule → dead letter.
    let dead = spawn_receiver().await;
    dead.respond_with.store(500, Ordering::SeqCst);
    let sub2 = events::create_subscription(
        &state.db,
        owner,
        serde_json::from_value(json!({
            "url": dead.url,
            "secret": "dead-secret-0123456789",
            "events": ["paper.decided"],
        }))
        .unwrap(),
    )
    .await
    .unwrap();
    let mut tx = state.db.begin().await.unwrap();
    events::emit(
        &mut tx,
        "paper.decided",
        json!({"paper_id": Uuid::new_v4()}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    for _ in 0..events::BACKOFF_SECS.len() {
        force_due(&state.db).await;
        events::run_once(&state.db, &http).await.unwrap();
    }
    let (status, attempt, code): (String, i32, Option<i32>) = sqlx::query_as(
        "SELECT status, attempt, last_status_code FROM webhook_deliveries WHERE subscription_id = $1",
    )
    .bind(sub2.id)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(status, "dead");
    assert_eq!(attempt as usize, events::BACKOFF_SECS.len());
    assert_eq!(code, Some(500));

    // Admin can inspect the dead letter through the API.
    let admin_key = mint_key(
        &state,
        &format!("it6-dl-admin-{}", Uuid::new_v4()),
        &[scope::ADMIN],
    )
    .await;
    let app = build_router(state.clone());
    let list = json_body(
        app.oneshot(req(
            "GET",
            "/api/v1/webhooks/deliveries?status=dead",
            &admin_key,
            None,
        ))
        .await
        .unwrap(),
    )
    .await;
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["subscription_id"] == json!(sub2.id)));
}

#[tokio::test]
async fn subscription_validation_and_admin_gate() {
    let state = setup(10_000).await;
    let _guard = webhook_lock().lock().await;
    quiesce_pipeline(&state.db).await;
    let admin_key = mint_key(
        &state,
        &format!("it6-sub-admin-{}", Uuid::new_v4()),
        &[scope::ADMIN],
    )
    .await;
    let user_key = mint_key(
        &state,
        &format!("it6-sub-user-{}", Uuid::new_v4()),
        &[scope::PAPERS_READ, scope::PAPERS_WRITE],
    )
    .await;
    let app = build_router(state.clone());

    // Non-admin cannot manage subscriptions.
    let f = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/webhooks/subscriptions",
            &user_key,
            Some(json!({"url": "https://x.test/h", "secret": "0123456789abcdef", "events": ["paper.decided"]})),
        ))
        .await
        .unwrap();
    assert_eq!(f.status(), StatusCode::FORBIDDEN);

    // Unknown event type rejected.
    let bad = app
        .clone()
        .oneshot(req(
            "POST",
            "/api/v1/webhooks/subscriptions",
            &admin_key,
            Some(json!({"url": "https://x.test/h", "secret": "0123456789abcdef", "events": ["paper.created"]})),
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    // Create → deactivate → deactivated subscription receives no fan-out.
    let created = json_body(
        app.clone()
            .oneshot(req(
                "POST",
                "/api/v1/webhooks/subscriptions",
                &admin_key,
                Some(json!({"url": "https://x.test/h", "secret": "0123456789abcdef", "events": ["paper.decided"]})),
            ))
            .await
            .unwrap(),
    )
    .await;
    let sub_id = created["id"].as_str().unwrap().to_string();
    let p = app
        .clone()
        .oneshot(req(
            "PATCH",
            &format!("/api/v1/webhooks/subscriptions/{sub_id}"),
            &admin_key,
            Some(json!({"active": false})),
        ))
        .await
        .unwrap();
    assert_eq!(p.status(), StatusCode::OK);

    let mut tx = state.db.begin().await.unwrap();
    events::emit(
        &mut tx,
        "paper.decided",
        json!({"paper_id": Uuid::new_v4()}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    events::fan_out(&state.db).await.unwrap();
    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM webhook_deliveries WHERE subscription_id = $1")
            .bind(Uuid::parse_str(&sub_id).unwrap())
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(n, 0, "inactive subscription must not receive deliveries");
}
