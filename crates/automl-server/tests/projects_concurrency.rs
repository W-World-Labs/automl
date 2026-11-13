//! Deterministic project-governance concurrency regressions for issue #33.
//! These tests hold a project row lock, wait until HTTP requests are visible
//! as PostgreSQL lock waiters, then commit the competing change.

use std::{str::FromStr, sync::Arc, time::Duration};

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
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use tower::ServiceExt;
use uuid::Uuid;
use w_id_sdk::SsoConfig;

const SCOPES: &[&str] = &[
    scope::PROJECTS_READ,
    scope::PROJECTS_WRITE,
    scope::PROJECTS_ADMIN,
];

fn config() -> Config {
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
            .join(format!("automl-project-concurrency-{}", Uuid::new_v4()))
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

async fn setup() -> (AppState, String) {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let app_name = format!("automl-project-test-{}", Uuid::new_v4());
    let opts = PgConnectOptions::from_str(&url)
        .expect("DATABASE_URL")
        .application_name(&app_name);
    let db = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(opts)
        .await
        .expect("connect");
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migrate");
    let cfg = config();
    let storage = automl_server::storage_init::build_storage(&cfg)
        .await
        .expect("storage");
    let state = AppState {
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
    };
    (state, app_name)
}

async fn key(state: &AppState) -> (String, Uuid, String) {
    let wid = format!("s-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let scholar = sso::ensure_scholar(&state.db, &wid, &wid).await.unwrap();
    let scopes: Vec<String> = SCOPES.iter().map(|s| (*s).to_owned()).collect();
    let created = keys::create_key(
        &state.db,
        scholar,
        CreateKeyRequest {
            name: format!("concurrency-{wid}"),
            scopes: scopes.clone(),
            expires_at: None,
        },
        &scopes,
        true,
    )
    .await
    .unwrap();
    (created.key, scholar, wid)
}

async fn scoped_key(state: &AppState, scholar: Uuid, scopes: &[&str]) -> String {
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();
    keys::create_key(
        &state.db,
        scholar,
        CreateKeyRequest {
            name: format!("project-writer-{}", Uuid::new_v4()),
            scopes: scopes.clone(),
            expires_at: None,
        },
        &scopes,
        true,
    )
    .await
    .unwrap()
    .key
}

async fn body(resp: axum::response::Response) -> Value {
    serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes())
        .unwrap_or(Value::Null)
}

async fn call(
    app: axum::Router,
    method: &'static str,
    uri: String,
    token: String,
    value: Option<Value>,
) -> StatusCode {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if value.is_some() {
        req = req.header("content-type", "application/json");
    }
    let resp = app
        .oneshot(
            req.body(Body::from(value.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

async fn create(app: &axum::Router, token: &str) -> Uuid {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/research-projects")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"title":"Concurrency fixture"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    Uuid::parse_str(body(resp).await["id"].as_str().unwrap()).unwrap()
}

async fn add_member(app: &axum::Router, pid: Uuid, token: &str, wid: &str, role: &str) -> Uuid {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/research-projects/{pid}/members"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"scholar":wid,"role":role}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    Uuid::parse_str(body(resp).await["id"].as_str().unwrap()).unwrap()
}

async fn hold_project(pool: &PgPool, pid: Uuid) -> sqlx::Transaction<'_, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM research_projects WHERE id=$1 FOR UPDATE")
        .bind(pid)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx
}

/// Wait for requests from this test's pool to be queued on a PostgreSQL lock.
/// Observing the lock wait (rather than sleeping a guessed duration) proves
/// the competing request reached the serialization point before release.
async fn wait_lock_waiters(pool: &PgPool, app_name: &str, expected: i64) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query ILIKE '%research_projects%'",
            ).bind(app_name).fetch_one(pool).await.unwrap();
            if count >= expected { break; }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }).await.expect("HTTP mutations did not queue behind the held project lock");
}

#[tokio::test]
async fn title_patch_cannot_resurrect_delete_or_restore_visibility() {
    let (state, app_name) = setup().await;
    let (token, scholar, _) = key(&state).await;
    let writer = scoped_key(&state, scholar, &[scope::PROJECTS_WRITE]).await;
    let app = build_router(state.clone());

    let deleted = create(&app, &token).await;
    let mut tx = hold_project(&state.db, deleted).await;
    let patch_app = app.clone();
    let patch_token = writer.clone();
    let patch = tokio::spawn(async move {
        call(
            patch_app,
            "PATCH",
            format!("/api/v1/research-projects/{deleted}"),
            patch_token,
            Some(json!({"title":"late title"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    sqlx::query("UPDATE research_projects SET status='deleted' WHERE id=$1")
        .bind(deleted)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(patch.await.unwrap(), StatusCode::NOT_FOUND);
    let status: String = sqlx::query_scalar("SELECT status FROM research_projects WHERE id=$1")
        .bind(deleted)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(status, "deleted");

    let visible = create(&app, &token).await;
    let mut tx = hold_project(&state.db, visible).await;
    let patch_app = app.clone();
    let patch_token = writer.clone();
    let patch = tokio::spawn(async move {
        call(
            patch_app,
            "PATCH",
            format!("/api/v1/research-projects/{visible}"),
            patch_token,
            Some(json!({"title":"renamed"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    sqlx::query("UPDATE research_projects SET visibility='public_meta' WHERE id=$1")
        .bind(visible)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(patch.await.unwrap(), StatusCode::OK);
    let row: (String, String) =
        sqlx::query_as("SELECT title, visibility FROM research_projects WHERE id=$1")
            .bind(visible)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(row, ("renamed".into(), "public_meta".into()));
    let title_audit: Value = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE project_id=$1 AND action='project.updated' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(visible)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(title_audit["changed"], json!(["title"]));
}

#[tokio::test]
async fn queued_lifecycle_transition_uses_committed_state_and_audit_origin() {
    let (state, app_name) = setup().await;
    let (token, _, _) = key(&state).await;
    let app = build_router(state.clone());
    let pid = create(&app, &token).await;
    sqlx::query("UPDATE research_projects SET status='active' WHERE id=$1")
        .bind(pid)
        .execute(&state.db)
        .await
        .unwrap();
    let tx = hold_project(&state.db, pid).await;
    let delete_app = app.clone();
    let delete_token = token.clone();
    let deleting = tokio::spawn(async move {
        call(
            delete_app,
            "PATCH",
            format!("/api/v1/research-projects/{pid}"),
            delete_token,
            Some(json!({"status":"deleted"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    let archive_app = app.clone();
    let archive_token = token.clone();
    let archiving = tokio::spawn(async move {
        call(
            archive_app,
            "PATCH",
            format!("/api/v1/research-projects/{pid}"),
            archive_token,
            Some(json!({"status":"archived"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 2).await;
    tx.commit().await.unwrap();
    assert_eq!(deleting.await.unwrap(), StatusCode::OK);
    assert_eq!(archiving.await.unwrap(), StatusCode::NOT_FOUND);
    let audits: Vec<(String, Value)> = sqlx::query_as("SELECT action, detail FROM audit_events WHERE project_id=$1 AND action='project.status_changed' ORDER BY created_at, id").bind(pid).fetch_all(&state.db).await.unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].1["from"], "active");
    assert_eq!(audits[0].1["to"], "deleted");

    let next = create(&app, &token).await;
    sqlx::query("UPDATE research_projects SET status='active' WHERE id=$1")
        .bind(next)
        .execute(&state.db)
        .await
        .unwrap();
    let tx = hold_project(&state.db, next).await;
    let archive_app = app.clone();
    let archive_token = token.clone();
    let archiving = tokio::spawn(async move {
        call(
            archive_app,
            "PATCH",
            format!("/api/v1/research-projects/{next}"),
            archive_token,
            Some(json!({"status":"archived"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    let delete_app = app.clone();
    let delete_token = token.clone();
    let deleting = tokio::spawn(async move {
        call(
            delete_app,
            "PATCH",
            format!("/api/v1/research-projects/{next}"),
            delete_token,
            Some(json!({"status":"deleted"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 2).await;
    tx.commit().await.unwrap();
    assert_eq!(archiving.await.unwrap(), StatusCode::OK);
    assert_eq!(deleting.await.unwrap(), StatusCode::OK);
    let transitions: Vec<Value> = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE project_id=$1 AND action='project.status_changed' ORDER BY created_at, id",
    )
    .bind(next)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[0]["from"], "active");
    assert_eq!(transitions[0]["to"], "archived");
    assert_eq!(transitions[1]["from"], "archived");
    assert_eq!(transitions[1]["to"], "deleted");
}

#[tokio::test]
async fn concurrent_owner_removals_preserve_one_owner() {
    let (state, app_name) = setup().await;
    let (token, caller_id, _) = key(&state).await;
    let (_, _, other_owner_wid) = key(&state).await;
    let app = build_router(state.clone());
    let pid = create(&app, &token).await;
    let other_owner = add_member(&app, pid, &token, &other_owner_wid, "owner").await;
    let caller_member: Uuid = sqlx::query_scalar(
        "SELECT id FROM project_members WHERE project_id=$1 AND scholar_id=$2 AND principal_type='scholar'",
    )
    .bind(pid)
    .bind(caller_id)
    .fetch_one(&state.db)
    .await
    .unwrap();

    let tx = hold_project(&state.db, pid).await;
    let app1 = app.clone();
    let key1 = token.clone();
    let first = tokio::spawn(async move {
        call(
            app1,
            "DELETE",
            format!("/api/v1/research-projects/{pid}/members/{other_owner}"),
            key1,
            None,
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    let app2 = app.clone();
    let key2 = token.clone();
    let second = tokio::spawn(async move {
        call(
            app2,
            "DELETE",
            format!("/api/v1/research-projects/{pid}/members/{caller_member}"),
            key2,
            None,
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 2).await;
    tx.commit().await.unwrap();
    let statuses = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::NO_CONTENT)
            .count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        1,
        "{statuses:?}"
    );
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM project_members WHERE project_id=$1 AND role='owner' AND principal_type='scholar'").bind(pid).fetch_one(&state.db).await.unwrap();
    assert_eq!(owners, 1);
}

#[tokio::test]
async fn removed_caller_cannot_run_queued_patch_add_or_remove() {
    let (state, app_name) = setup().await;
    let (caller, caller_id, _) = key(&state).await;
    let (_, _, owner_wid) = key(&state).await;
    let (_, _, target_wid) = key(&state).await;
    let (_, _, add_wid) = key(&state).await;
    let app = build_router(state.clone());
    let pid = create(&app, &caller).await;
    let _second_owner = add_member(&app, pid, &caller, &owner_wid, "owner").await;
    let target = add_member(&app, pid, &caller, &target_wid, "member").await;
    let audit_count_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE project_id=$1")
            .bind(pid)
            .fetch_one(&state.db)
            .await
            .unwrap();
    let mut tx = hold_project(&state.db, pid).await;

    let patch_app = app.clone();
    let patch_key = caller.clone();
    let patch = tokio::spawn(async move {
        call(
            patch_app,
            "PATCH",
            format!("/api/v1/research-projects/{pid}"),
            patch_key,
            Some(json!({"title":"unauthorized"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    let add_app = app.clone();
    let add_key = caller.clone();
    let add_wid_copy = add_wid.clone();
    let adding = tokio::spawn(async move {
        call(
            add_app,
            "POST",
            format!("/api/v1/research-projects/{pid}/members"),
            add_key,
            Some(json!({"scholar":add_wid_copy,"role":"member"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 2).await;
    let remove_app = app.clone();
    let remove_key = caller.clone();
    let removing = tokio::spawn(async move {
        call(
            remove_app,
            "DELETE",
            format!("/api/v1/research-projects/{pid}/members/{target}"),
            remove_key,
            None,
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 3).await;

    sqlx::query("DELETE FROM project_members WHERE project_id=$1 AND principal_type='scholar' AND scholar_id=$2")
        .bind(pid).bind(caller_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for status in [
        patch.await.unwrap(),
        adding.await.unwrap(),
        removing.await.unwrap(),
    ] {
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let title: String = sqlx::query_scalar("SELECT title FROM research_projects WHERE id=$1")
        .bind(pid)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(title, "Concurrency fixture");
    let target_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_members WHERE id=$1)")
            .bind(target)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(target_exists);
    let added: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_members m JOIN scholars s ON s.id=m.scholar_id WHERE m.project_id=$1 AND s.w_id=$2)").bind(pid).bind(add_wid).fetch_one(&state.db).await.unwrap();
    assert!(!added);
    let audit_count_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE project_id=$1")
            .bind(pid)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(audit_count_after, audit_count_before);
}

#[tokio::test]
async fn queued_membership_mutations_reject_deleted_project() {
    let (state, app_name) = setup().await;
    let (token, _, _) = key(&state).await;
    let (_, _, target_wid) = key(&state).await;
    let (_, _, new_wid) = key(&state).await;
    let app = build_router(state.clone());
    let pid = create(&app, &token).await;
    let target = add_member(&app, pid, &token, &target_wid, "member").await;
    let mut tx = hold_project(&state.db, pid).await;
    let add_app = app.clone();
    let add_token = token.clone();
    let wid = new_wid.clone();
    let adding = tokio::spawn(async move {
        call(
            add_app,
            "POST",
            format!("/api/v1/research-projects/{pid}/members"),
            add_token,
            Some(json!({"scholar":wid,"role":"member"})),
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 1).await;
    let remove_app = app.clone();
    let remove_token = token.clone();
    let removing = tokio::spawn(async move {
        call(
            remove_app,
            "DELETE",
            format!("/api/v1/research-projects/{pid}/members/{target}"),
            remove_token,
            None,
        )
        .await
    });
    wait_lock_waiters(&state.db, &app_name, 2).await;
    sqlx::query("UPDATE research_projects SET status='deleted' WHERE id=$1")
        .bind(pid)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(adding.await.unwrap(), StatusCode::NOT_FOUND);
    assert_eq!(removing.await.unwrap(), StatusCode::NOT_FOUND);
    let member_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM project_members WHERE project_id=$1")
            .bind(pid)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(member_count, 2);
}
