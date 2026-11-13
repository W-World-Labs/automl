//! Formal end-to-end acceptance (docs/ACCEPTANCE.md).
//!
//! Spawns the REAL automl-server binary over real HTTP with a mock Gitea and a
//! local webhook receiver, then drives the product journeys from
//! docs/PRODUCT-FLOWS.md — including the real `automl` CLI binary for the
//! loopback login, device registration and a full reproduce run against a
//! real local git repository.
//!
//! Requires DATABASE_URL (same contract as the integration suites) and `git`.

use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{
    http::StatusCode,
    routing::{any, post},
    Router,
};
use serde_json::{json, Value};

const JWT_SECRET: &str = "acceptance-jwt-secret-32-bytes-min!!";
const WEBHOOK_SECRET: &str = "acceptance-webhook-secret";
const GITEA_HOOK_SECRET: &str = "acceptance-gitea-hook-secret";

struct Receiver {
    url: String,
    hits: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<(String, String)>>>, // (signature, body)
}

async fn spawn_receiver() -> Receiver {
    let hits = Arc::new(AtomicUsize::new(0));
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let (h, s) = (hits.clone(), seen.clone());
    let app = Router::new().route(
        "/hook",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let (h, s) = (h.clone(), s.clone());
            async move {
                h.fetch_add(1, Ordering::SeqCst);
                let sig = headers
                    .get("x-automl-signature")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                s.lock().unwrap().push((sig, body));
                StatusCode::OK
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Receiver { url, hits, seen }
}

async fn spawn_mock_gitea() -> (String, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let app = Router::new().route(
        "/api/v1/orgs/{org}/repos",
        any(move || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                StatusCode::CREATED
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, calls)
}

fn server_binary() -> std::path::PathBuf {
    // target/debug/deps/acceptance-* → target/debug/automl-server
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("automl-server")
}

fn session_cookie(sub: &str, admin: bool) -> String {
    let cfg = w_id_sdk::SsoConfig::new(
        "http://127.0.0.1:1",
        "acceptance",
        "acceptance-secret",
        "http://127.0.0.1:1/cb",
        JWT_SECRET,
    );
    let user = w_id_sdk::SsoUser {
        workspace_id: None,
        sub: sub.into(),
        login: Some(sub.into()),
        email: None,
        name: None,
        picture: None,
        role: None,
        is_platform_admin: admin,
        workspaces: vec![],
        demo_workspaces: vec![],
        tier: None,
        workspace_tier: None,
        product: None,
        quotas: Default::default(),
        entitlements: vec![],
    };
    format!("automl_session={}", cfg.issue_session(&user).unwrap())
}

struct Harness {
    base: String,
    http: reqwest::Client,
    scholar: String,
    admin: String,
    _server: tokio::process::Child,
}

impl Harness {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        cookie: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("cookie", cookie);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.expect("request");
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let v = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, v)
    }

    async fn get(&self, path: &str, cookie: &str) -> (u16, Value) {
        self.call(reqwest::Method::GET, path, cookie, None).await
    }
    async fn post(&self, path: &str, cookie: &str, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, cookie, Some(body))
            .await
    }
    async fn patch(&self, path: &str, cookie: &str, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::PATCH, path, cookie, Some(body))
            .await
    }

    /// Poll until `pred` holds on GET `path` (acceptance uses 1s worker ticks).
    async fn wait_until(
        &self,
        path: &str,
        cookie: &str,
        pred: impl Fn(&Value) -> bool,
        what: &str,
    ) -> Value {
        for _ in 0..40 {
            let (_, v) = self.get(path, cookie).await;
            if pred(&v) {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("timed out waiting for: {what}");
    }
}

/// The acceptance run owns a dedicated database (`<name>_accept`) so the
/// concurrently running integration suites (which quiesce shared webhook
/// tables) can never interfere with journey assertions.
async fn acceptance_database() -> String {
    let base_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let (prefix, name) = base_url
        .rsplit_once('/')
        .expect("DATABASE_URL has a database name");
    let accept_name = format!("{name}_accept");
    assert!(
        accept_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "acceptance database name must be a plain SQL identifier"
    );
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&base_url)
        .await
        .expect("connect for db provisioning");
    // CREATE DATABASE cannot run in a tx; duplicates are fine.
    // SQLx 0.9 requires an explicit audit marker for dynamic SQL. The
    // identifier is constrained above because CREATE DATABASE cannot bind it.
    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE DATABASE {accept_name}"
    )))
    .execute(&admin)
    .await;
    format!("{prefix}/{accept_name}")
}

async fn start_harness(gitea_url: &str) -> Harness {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = acceptance_database().await;
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let base = format!("http://127.0.0.1:{port}");
    let storage_root = std::env::temp_dir().join(format!("automl-accept-{port}"));

    let bin = server_binary();
    assert!(
        bin.exists(),
        "server binary missing at {bin:?} — run via cargo test --workspace"
    );
    let server = tokio::process::Command::new(bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env("DATABASE_URL", &db)
        .env("BIND_ADDR", format!("127.0.0.1:{port}"))
        .env("ALLOWED_ORIGINS", &base)
        .env("STORAGE_BACKEND", "local")
        .env("STORAGE_LOCAL_ROOT", &storage_root)
        .env("W_ID_SSO_URL", "http://127.0.0.1:1")
        .env("W_ID_CLIENT_ID", "acceptance")
        .env("W_ID_CLIENT_SECRET", "acceptance-secret")
        .env("W_ID_REDIRECT_URI", format!("{base}/auth/callback"))
        .env("JWT_SECRET", JWT_SECRET)
        .env("GITEA_URL", gitea_url)
        .env("GITEA_TOKEN", "acceptance-token")
        .env("GITEA_ORG", "papers")
        .env("GITEA_WEBHOOK_SECRET", GITEA_HOOK_SECRET)
        .env("WEBHOOK_TICK_SECS", "1")
        .env("GITEA_SWEEP_SECS", "1")
        .env("RATE_LIMIT_RPS", "10000")
        .env("RATE_LIMIT_BURST", "10000")
        .env("RATE_LIMIT_WRITE_RPS", "10000")
        .env("RATE_LIMIT_WRITE_BURST", "10000")
        .env("RATE_LIMIT_FEEDBACK_RPS", "10000")
        .env("RATE_LIMIT_FEEDBACK_BURST", "10000")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn automl-server");

    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

    // Wait for /health.
    for i in 0..60 {
        if let Ok(r) = http.get(format!("{base}/api/v1/health")).send().await {
            if r.status().is_success() {
                break;
            }
        }
        assert!(i < 59, "server did not come up");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Retire leftovers from PREVIOUS acceptance runs (we own this database;
    // the server has already migrated it during boot).
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&db)
        .await
        .expect("connect accept db");
    let _ = sqlx::query("UPDATE webhook_subscriptions SET active = false")
        .execute(&pool)
        .await;
    let _ = sqlx::query("UPDATE webhook_deliveries SET status = 'dead' WHERE status = 'pending'")
        .execute(&pool)
        .await;

    let run = uuid::Uuid::new_v4().simple().to_string();
    Harness {
        base,
        http,
        scholar: session_cookie(&format!("accept-scholar-{run}"), false),
        admin: session_cookie(&format!("accept-admin-{run}"), true),
        _server: server,
    }
}

/// Build a local git repo with a reproduce spec + script; returns (path, HEAD sha).
fn git_fixture() -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("automl-accept-repo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("reproduce.yaml"),
        "steps:\n  - run: bash reproduce.sh\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("reproduce.sh"),
        "#!/usr/bin/env bash\necho running acceptance experiment\necho 'METRICS accuracy=0.93 loss=0.07'\n",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .current_dir(&dir)
            .env("GIT_AUTHOR_NAME", "acc")
            .env("GIT_AUTHOR_EMAIL", "acc@test")
            .env("GIT_COMMITTER_NAME", "acc")
            .env("GIT_COMMITTER_EMAIL", "acc@test")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "acceptance fixture"]);
    let sha = git(&["rev-parse", "HEAD"]);
    (dir, sha)
}

#[tokio::test(flavor = "multi_thread")]
async fn full_product_acceptance() {
    let (gitea_url, gitea_calls) = spawn_mock_gitea().await;
    let receiver = spawn_receiver().await;
    let h = start_harness(&gitea_url).await;
    let (repo_dir, fixture_sha) = git_fixture();
    let cli_config_dir =
        std::env::temp_dir().join(format!("automl-accept-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cli_config_dir);

    // Quiesce leftovers from earlier suites sharing the DB.
    let (status, _) = h
        .post(
            "/api/v1/webhooks/subscriptions",
            &h.admin,
            json!({"url": receiver.url, "secret": WEBHOOK_SECRET,
                   "events": ["paper.decided", "repro.completed"]}),
        )
        .await;
    assert_eq!(status, 201, "[J4] admin registers webhook subscription");

    // ── J1: scholar publishes a platform-hosted paper and it gets decided ──
    let (status, paper) = h
        .post(
            "/api/v1/papers",
            &h.scholar,
            json!({"title": "Acceptance: platform-hosted paper", "abstract": "walk J1"}),
        )
        .await;
    assert_eq!(status, 201, "[J1] create paper");
    assert_eq!(
        paper["code_status"], "repo_pending",
        "[J1] starts repo_pending"
    );
    let paper_id = paper["id"].as_str().unwrap().to_string();

    // Provisioning (inline attempt or 1s sweep) flips repo_pending → repo_ready.
    h.wait_until(
        &format!("/api/v1/papers/{paper_id}"),
        &h.scholar,
        |p| p["code_status"] == "repo_ready",
        "gitea repo provisioned (repo_ready)",
    )
    .await;
    assert!(
        gitea_calls.load(Ordering::SeqCst) >= 1,
        "[J1] mock Gitea saw create-repo"
    );

    // Push webhook (signed) → ready + latest_commit synced.
    let push = json!({"repository": {"name": paper_id}, "after": fixture_sha}).to_string();
    let sig = w_crypto::hmac_sha256_hex(GITEA_HOOK_SECRET.as_bytes(), push.as_bytes());
    let resp = h
        .http
        .post(format!("{}/api/v1/gitea/events", h.base))
        .header("x-gitea-event", "push")
        .header("x-gitea-signature", sig)
        .header("content-type", "application/json")
        .body(push)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 204, "[J1] signed push accepted");
    let p = h
        .get(&format!("/api/v1/papers/{paper_id}"), &h.scholar)
        .await
        .1;
    assert_eq!(p["code_status"], "ready", "[J1] push → ready");
    assert_eq!(
        p["latest_commit"],
        json!(fixture_sha),
        "[J1] latest_commit synced"
    );

    // Author pins the commit and submits.
    let (status, _) = h
        .patch(
            &format!("/api/v1/papers/{paper_id}"),
            &h.scholar,
            json!({"commit_sha": fixture_sha}),
        )
        .await;
    assert_eq!(status, 200, "[J1] pin commit");
    let (status, _) = h
        .patch(
            &format!("/api/v1/papers/{paper_id}"),
            &h.scholar,
            json!({"review_status": "submitted"}),
        )
        .await;
    assert_eq!(status, 200, "[J1] author submits");

    // Human gate is admin-only: author cannot advance or decide.
    let (status, v) = h
        .patch(
            &format!("/api/v1/papers/{paper_id}"),
            &h.scholar,
            json!({"review_status": "accepted"}),
        )
        .await;
    assert_eq!(status, 403, "[J1] author cannot decide: {v}");

    // Admin advances, reviews, decides.
    let (status, _) = h
        .patch(
            &format!("/api/v1/papers/{paper_id}"),
            &h.admin,
            json!({"review_status": "under_review"}),
        )
        .await;
    assert_eq!(status, 200, "[J1] admin advances to under_review");
    let (status, _) = h
        .post(
            &format!("/api/v1/papers/{paper_id}/reviews"),
            &h.admin,
            json!({"verdict": "approve", "body": "reproduces cleanly, accept"}),
        )
        .await;
    assert_eq!(status, 201, "[J1] review recorded");
    let (status, decided) = h
        .patch(
            &format!("/api/v1/papers/{paper_id}"),
            &h.admin,
            json!({"review_status": "accepted"}),
        )
        .await;
    assert_eq!(status, 200, "[J1] human gate decides");
    assert_eq!(decided["review_status"], "accepted");

    // Terminal state: no way back (state machine).
    let (status, _) = h
        .patch(
            &format!("/api/v1/papers/{paper_id}"),
            &h.admin,
            json!({"review_status": "draft"}),
        )
        .await;
    assert_eq!(status, 409, "[SM] accepted is terminal");

    // paper.decided arrives at the receiver, signed.
    for _ in 0..30 {
        if receiver.hits.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let seen = receiver.seen.lock().unwrap().clone();
    let decided_evt = seen
        .iter()
        .find(|(_, body)| body.contains("paper.decided") && body.contains(&paper_id))
        .expect("[J1] paper.decided delivered");
    let expected = format!(
        "sha256={}",
        w_crypto::hmac_sha256_hex(WEBHOOK_SECRET.as_bytes(), decided_evt.1.as_bytes())
    );
    assert_eq!(
        decided_evt.0, expected,
        "[J1] receiver-verified HMAC signature"
    );
    let evt: Value = serde_json::from_str(&decided_evt.1).unwrap();
    assert_eq!(evt["data"]["decision"], "accepted");
    assert!(
        evt["event_id"].as_str().is_some(),
        "[J1] event_id for idempotency"
    );

    // ── J5: real CLI loopback login (harness plays the browser) ──
    let cli = env!("CARGO_BIN_EXE_automl");
    let mut login = tokio::process::Command::new(cli)
        .env("AUTOML_CONFIG_DIR", &cli_config_dir)
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .args(["--server", &h.base, "--format", "json", "login"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn automl login");

    // Read stderr until the printed start URL appears.
    let mut stderr = login.stderr.take().unwrap();
    let start_url = {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut url = None;
        for _ in 0..100 {
            let mut chunk = [0u8; 4096];
            tokio::select! {
                n = stderr.read(&mut chunk) => {
                    let n = n.unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf);
                    if let Some(line) = text.lines().find(|l| l.trim().starts_with("http")) {
                        url = Some(line.trim().to_string());
                        break;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
        url.expect("[J5] CLI printed the start URL")
    };

    // Browser role: hit /cli/auth/start with the scholar session; expect a
    // redirect to the CLI's loopback; follow it so the CLI receives the code.
    let resp = h
        .http
        .get(&start_url)
        .header("cookie", &h.scholar)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection(), "[J5] start redirects");
    let loopback = resp.headers()["location"].to_str().unwrap().to_string();
    assert!(
        loopback.starts_with("http://127.0.0.1:"),
        "[J5] loopback target: {loopback}"
    );
    let done = h.http.get(&loopback).send().await.unwrap();
    assert!(
        done.status().is_success(),
        "[J5] loopback callback answered"
    );

    let out = tokio::time::timeout(Duration::from_secs(20), login.wait_with_output())
        .await
        .expect("[J5] login finished")
        .unwrap();
    assert!(out.status.success(), "[J5] automl login exit 0");
    let creds_path = cli_config_dir.join("credentials.toml");
    assert!(creds_path.exists(), "[J5] credentials saved");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&creds_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "[J5] credentials are 0600");
    }

    let run_cli = |args: Vec<String>| {
        let cli = cli.to_string();
        let dir = cli_config_dir.clone();
        let base = h.base.clone();
        async move {
            let out = tokio::process::Command::new(cli)
                .env("AUTOML_CONFIG_DIR", &dir)
                .env("HOME", std::env::var("HOME").unwrap_or_default())
                .args(["--server", &base, "--format", "json"])
                .args(&args)
                .output()
                .await
                .unwrap();
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
        }
    };

    // ── Research Hub P1: real CLI paper-add preserves normalized shelf data ──
    let (status, project) = h
        .post(
            "/api/v1/research-projects",
            &h.scholar,
            json!({"title": "Acceptance research shelf", "summary": "P1 CLI journey", "visibility": "private"}),
        )
        .await;
    assert_eq!(status, 201, "[P1] create project: {project}");
    let project_id = project["id"].as_str().unwrap().to_string();
    let (code, out, err) = run_cli(vec![
        "project".into(),
        "paper-add".into(),
        project_id.clone(),
        "--doi".into(),
        "https://doi.org/10.1234/EXAMPLE".into(),
        "--fuel-role".into(),
        "baseline".into(),
        "--notes".into(),
        "acceptance seed".into(),
        "--tags".into(),
        "seed,transformer".into(),
    ])
    .await;
    assert_eq!(code, 0, "[P1] CLI paper-add: {err}");
    let added: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(added["doi"], "10.1234/example", "[P1] normalized DOI");
    assert_eq!(added["fuel_role"], "baseline");
    assert_eq!(added["notes"], "acceptance seed");
    assert_eq!(added["tags"], json!(["seed", "transformer"]));
    let (status, shelf) = h
        .get(
            &format!("/api/v1/research-projects/{project_id}/papers"),
            &h.scholar,
        )
        .await;
    assert_eq!(status, 200, "[P1] list project papers: {shelf}");
    assert_eq!(shelf["data"].as_array().unwrap().len(), 1);
    assert_eq!(shelf["data"][0]["doi"], "10.1234/example");

    // ── Research Hub P2: real CLI bind/assets/search over the stub provider ──
    let (code, out, err) = run_cli(vec![
        "project".into(),
        "bind-daas".into(),
        project_id.clone(),
        "--workspace-id".into(),
        "ws-acceptance".into(),
    ])
    .await;
    assert_eq!(code, 0, "[P2] CLI bind-daas: {err}");
    let bound: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(bound["workspace_id"], "ws-acceptance");
    assert_eq!(bound["provider_name"], "stub");
    assert_eq!(bound["verified"], false);

    let (code, out, err) = run_cli(vec![
        "project".into(),
        "asset-add".into(),
        project_id.clone(),
        "--kind".into(),
        "note".into(),
        "--title".into(),
        "kickoff".into(),
        "--metadata".into(),
        r#"{"body":"acceptance note"}"#.into(),
    ])
    .await;
    assert_eq!(code, 0, "[P2] CLI asset-add: {err}");
    let asset: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(asset["kind"], "note");
    assert_eq!(asset["metadata"]["body"], "acceptance note");

    let (code, out, err) = run_cli(vec![
        "project".into(),
        "import-pdf".into(),
        project_id.clone(),
        "--title".into(),
        "acceptance.pdf".into(),
    ])
    .await;
    assert_eq!(code, 0, "[P2] CLI import-pdf: {err}");
    let pdf: Value = serde_json::from_str(&out).unwrap();
    assert!(pdf["daas_document_id"]
        .as_str()
        .unwrap()
        .starts_with("stub-doc-"));
    assert_eq!(pdf["cached_title"], "acceptance.pdf");

    let (code, out, err) = run_cli(vec![
        "project".into(),
        "search".into(),
        project_id.clone(),
        "--query".into(),
        "attention".into(),
        "--persist".into(),
    ])
    .await;
    assert_eq!(code, 0, "[P2] CLI search --persist: {err}");
    let search: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(search["provider"], "stub");
    assert!(search["snapshot_asset_id"].as_str().is_some());

    let (status, assets) = h
        .get(
            &format!("/api/v1/research-projects/{project_id}/assets"),
            &h.scholar,
        )
        .await;
    assert_eq!(status, 200, "[P2] list project assets: {assets}");
    assert_eq!(assets["data"].as_array().unwrap().len(), 2);

    let (code, _, err) = run_cli(vec![
        "project".into(),
        "unbind-daas".into(),
        project_id.clone(),
    ])
    .await;
    assert_eq!(code, 0, "[P2] CLI unbind-daas: {err}");
    let (status, _) = h
        .get(
            &format!("/api/v1/research-projects/{project_id}/daas/binding"),
            &h.scholar,
        )
        .await;
    assert_eq!(status, 404, "[P2] binding gone after unbind");

    // ── J2: agent identity + signed reproduce run via the real CLI ──
    let (status, agent) = h
        .post(
            "/api/v1/agents",
            &h.scholar,
            json!({"name": "acceptance-agent"}),
        )
        .await;
    assert_eq!(status, 201, "[J2] agent registered: {agent}");
    let agent_id = agent["id"].as_str().unwrap().to_string();

    let (code, out, err) = run_cli(vec![
        "device".into(),
        "register".into(),
        "--agent".into(),
        agent_id.clone(),
    ])
    .await;
    assert_eq!(code, 0, "[J2] device register: {err}");
    let device: Value = serde_json::from_str(&out).unwrap();
    assert!(device["device_id"].as_str().is_some());

    // External-pointer paper reproducible from the local fixture repo.
    let (status, target) = h
        .post(
            "/api/v1/papers",
            &h.scholar,
            json!({
                "title": "Acceptance: reproducible paper（中文标题验证）",
                "repo_url": repo_dir.to_string_lossy(),
                "commit_sha": fixture_sha,
            }),
        )
        .await;
    assert_eq!(status, 201, "[J2] external paper created");
    let target_id = target["id"].as_str().unwrap().to_string();

    let (code, out, err) = run_cli(vec!["reproduce".into(), "run".into(), target_id.clone()]).await;
    assert_eq!(code, 0, "[J2] reproduce run: {err}");
    let result: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        result["run"]["status"], "accepted",
        "[J2] evidence accepted: {result}"
    );
    assert_eq!(
        result["evidence"]["signature_valid"], true,
        "[J2] device-signed evidence verified"
    );
    assert_eq!(
        result["evidence"]["metrics"]["accuracy"], 0.93,
        "[J2] metrics extracted"
    );

    // ── J3: reader sees the trust aggregate ──
    let (status, summary) = h
        .get(
            &format!("/api/v1/papers/{target_id}/reproduction"),
            &h.scholar,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(summary["runs_total"], 1, "[J3] {summary}");
    assert_eq!(
        summary["verified_by_n"], 1,
        "[J3] verified-by-1 from signed run"
    );

    // repro.completed delivered too.
    for _ in 0..30 {
        if receiver
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|(_, b)| b.contains("repro.completed"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        receiver
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|(_, b)| b.contains("repro.completed")),
        "[J2] repro.completed delivered"
    );

    // CLI read commands over the wire format (+ CJK title in table mode).
    let (code, out, _) = run_cli(vec![
        "papers".into(),
        "list".into(),
        "--limit".into(),
        "10".into(),
    ])
    .await;
    assert_eq!(code, 0);
    assert!(
        serde_json::from_str::<Value>(&out).unwrap()["data"].is_array(),
        "[J2] papers list --json wire shape"
    );
    let (code, _, _) = run_cli(vec![
        "runs".into(),
        "list".into(),
        "--paper".into(),
        target_id.clone(),
    ])
    .await;
    assert_eq!(code, 0, "[J2] runs list");
    let table = tokio::process::Command::new(cli)
        .env("AUTOML_CONFIG_DIR", &cli_config_dir)
        .args(["--server", &h.base, "papers", "list", "--limit", "10"])
        .output()
        .await
        .unwrap();
    assert!(table.status.success(), "[J2] CJK table render");

    // ── J4: community ──
    let (_, cats) = h.get("/api/v1/forum/categories?limit=10", &h.scholar).await;
    let cat_id = cats["data"][0]["id"].as_str().unwrap().to_string();
    let (status, thread) = h
        .post(
            "/api/v1/forum/threads",
            &h.scholar,
            json!({"category_id": cat_id, "title": "Acceptance thread", "body": "**hello**"}),
        )
        .await;
    assert_eq!(status, 201, "[J4] thread created");
    let thread_id = thread["id"].as_str().unwrap().to_string();
    let (status, _) = h
        .post(
            &format!("/api/v1/forum/threads/{thread_id}/posts"),
            &h.admin,
            json!({"body": "reply"}),
        )
        .await;
    assert_eq!(status, 201, "[J4] reply posted");

    let anon = h
        .http
        .post(format!("{}/api/v1/feedback", h.base))
        .json(&json!({"category": "feature", "body": "acceptance feedback"}))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status().as_u16(), 201, "[J4] anonymous feedback");
    let fb: Value = anon.json().await.unwrap();
    let fb_id = fb["id"].as_str().unwrap();
    let (status, _) = h
        .patch(
            &format!("/api/v1/admin/feedback/{fb_id}"),
            &h.admin,
            json!({"status": "triaged"}),
        )
        .await;
    assert_eq!(status, 200, "[J4] admin triage");
    // State machine: triaged → new is illegal.
    let (status, _) = h
        .patch(
            &format!("/api/v1/admin/feedback/{fb_id}"),
            &h.admin,
            json!({"status": "new"}),
        )
        .await;
    assert_eq!(status, 409, "[SM] triage cannot go back to new");

    // Stub-is-501 spot check.
    let (status, v) = h.get("/api/v1/notifications", &h.scholar).await;
    assert_eq!(status, 501, "[J4] notifications honestly 501: {v}");

    // ── logout: credentials removed, key revoked ──
    let (code, out, _) = run_cli(vec!["logout".into()]).await;
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap()["status"],
        "logged_out"
    );
    assert!(!creds_path.exists(), "[J5] credentials removed on logout");

    // Cleanup fixtures.
    let _ = std::fs::remove_dir_all(&repo_dir);
    let _ = std::fs::remove_dir_all(&cli_config_dir);

    println!("ACCEPTANCE: all journeys passed (J1 publish/decide+webhook, J2 agent reproduce signed, J3 verified-by-N, J4 community, J5 loopback login/logout, SM terminal-state checks)");
}
