//! P2 project DaaS binding/asset/search integration tests. Requires DATABASE_URL.

use std::sync::Arc;

use automl_core::scope;
use automl_server::{
    auth::{
        keys::{self, CreateKeyRequest},
        sso,
    },
    build_router,
    config::{Config, DaasConfig, EnrichmentConfig, RateLimitConfig},
    daas::Daas,
    paper_enrichment::PaperEnrichment,
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
use w_id_sdk::SsoConfig;

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
    let cfg = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: url,
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
            .join(format!("automl-daas-objects-{}", Uuid::new_v4()))
            .to_string_lossy()
            .into(),
        storage_public_base: "http://127.0.0.1:8030".into(),
        gitea: None,
        cli_dist_base: "https://dist.example.test".into(),
        public_base_url: "https://accept.example.test".into(),
        paper_enrichment: EnrichmentConfig::default(),
        daas: DaasConfig::default(),
    };
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
        paper_enrichment: Arc::new(PaperEnrichment::stub()),
        daas: Arc::new(Daas::stub()),
    }
}

async fn body(resp: axum::response::Response) -> Value {
    serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes())
        .unwrap_or(Value::Null)
}

async fn mint_key(state: &AppState, scopes: &[&str]) -> (String, Uuid, String) {
    let wid = format!("s-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let scholar = sso::ensure_scholar(&state.db, &wid, &wid).await.unwrap();
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).to_string()).collect();
    let created = keys::create_key(
        &state.db,
        scholar,
        CreateKeyRequest {
            name: format!("k-{wid}"),
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

const ALL_PROJECT_SCOPES: &[&str] = &[
    scope::PROJECTS_READ,
    scope::PROJECTS_WRITE,
    scope::PROJECTS_ADMIN,
];

async fn call(
    app: axum::Router,
    method: &str,
    uri: String,
    key: &str,
    value: Option<Value>,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {key}"));
    if value.is_some() {
        req = req.header("content-type", "application/json");
    }
    app.oneshot(
        req.body(Body::from(value.map(|v| v.to_string()).unwrap_or_default()))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn create_project(app: axum::Router, key: &str, title: &str) -> String {
    let resp = call(
        app,
        "POST",
        "/api/v1/research-projects".into(),
        key,
        Some(json!({"title": title})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    body(resp).await["id"].as_str().unwrap().to_owned()
}

async fn add_member(app: axum::Router, owner: &str, pid: &str, wid: &str, role: &str) {
    let resp = call(
        app,
        "POST",
        format!("/api/v1/research-projects/{pid}/members"),
        owner,
        Some(json!({"scholar": wid, "role": role})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

fn binding_uri(pid: &str) -> String {
    format!("/api/v1/research-projects/{pid}/daas/binding")
}

fn assets_uri(pid: &str) -> String {
    format!("/api/v1/research-projects/{pid}/assets")
}

#[tokio::test]
async fn binding_lifecycle_and_audit() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state.clone());
    let pid = create_project(app.clone(), &owner, "Binding lifecycle").await;

    // Stub bind: honest unverified pointer; project column stays in sync.
    let resp = call(
        app.clone(),
        "PUT",
        binding_uri(&pid),
        &owner,
        Some(json!({"workspace_id": "ws-alpha"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bound = body(resp).await;
    assert_eq!(bound["workspace_id"], "ws-alpha");
    assert_eq!(bound["provider"], "w_daas");
    assert_eq!(bound["provider_name"], "stub");
    assert_eq!(bound["verified"], false);
    assert_eq!(bound["status"], "active");

    let resp = call(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(body(resp).await["w_daas_workspace_id"], "ws-alpha");

    // Rebind retires the previous row and points the column at the new id.
    let resp = call(
        app.clone(),
        "PUT",
        binding_uri(&pid),
        &owner,
        Some(json!({"workspace_id": "ws-beta"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = call(app.clone(), "GET", binding_uri(&pid), &owner, None).await;
    assert_eq!(body(resp).await["workspace_id"], "ws-beta");
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_bindings WHERE project_id=$1 AND status='active'",
    )
    .bind(Uuid::parse_str(&pid).unwrap())
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(active, 1, "one live binding per project");
    let retired: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_bindings WHERE project_id=$1 AND status='unbound'",
    )
    .bind(Uuid::parse_str(&pid).unwrap())
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(retired, 1);

    let resp = call(app.clone(), "DELETE", binding_uri(&pid), &owner, None).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = call(app.clone(), "GET", binding_uri(&pid), &owner, None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let resp = call(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}"),
        &owner,
        None,
    )
    .await;
    assert!(body(resp).await["w_daas_workspace_id"].is_null());
    let resp = call(app.clone(), "DELETE", binding_uri(&pid), &owner, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "second unbind is honest"
    );

    for bad in [
        json!({"workspace_id": "  "}),
        json!({"workspace_id": ""}),
        json!({}),
    ] {
        let resp = call(app.clone(), "PUT", binding_uri(&pid), &owner, Some(bad)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    let pid_uuid = Uuid::parse_str(&pid).unwrap();
    for (action, want) in [("binding.bound", 2), ("binding.unbound", 1)] {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_events WHERE project_id=$1 AND action=$2",
        )
        .bind(pid_uuid)
        .bind(action)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(count, want, "{action} audit rows");
    }
}

#[tokio::test]
async fn binding_acl_enforced() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (member, _, member_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (viewer, _, viewer_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (outsider, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (read_only, _, _) = mint_key(&state, &[scope::PROJECTS_READ]).await;
    let app = build_router(state.clone());
    let pid = create_project(app.clone(), &owner, "Binding ACL").await;
    add_member(app.clone(), &owner, &pid, &member_wid, "member").await;
    add_member(app.clone(), &owner, &pid, &viewer_wid, "viewer").await;

    for (key, label) in [
        (&member, "member"),
        (&viewer, "viewer"),
        (&outsider, "outsider"),
    ] {
        let resp = call(
            app.clone(),
            "PUT",
            binding_uri(&pid),
            key,
            Some(json!({"workspace_id": "ws-x"})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{label} cannot bind");
    }
    let resp = call(
        app.clone(),
        "PUT",
        binding_uri(&pid),
        &read_only,
        Some(json!({"workspace_id": "ws-x"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "needs projects:admin");

    let resp = call(
        app.clone(),
        "PUT",
        binding_uri(&pid),
        &owner,
        Some(json!({"workspace_id": "ws-x"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    // Viewer+ can read the binding; outsiders cannot.
    let resp = call(app.clone(), "GET", binding_uri(&pid), &viewer, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = call(app.clone(), "GET", binding_uri(&pid), &outsider, None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // Unbind is owner-only too.
    let resp = call(app.clone(), "DELETE", binding_uri(&pid), &member, None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = call(app.clone(), "DELETE", binding_uri(&pid), &owner, None).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn asset_crud_acl_and_kind_validation() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (member, _, member_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (viewer, _, viewer_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (outsider, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state.clone());
    let pid = create_project(app.clone(), &owner, "Asset surface").await;
    add_member(app.clone(), &owner, &pid, &member_wid, "member").await;
    add_member(app.clone(), &owner, &pid, &viewer_wid, "viewer").await;

    // Metadata-only note is a content kind — no pointer required.
    let resp = call(
        app.clone(),
        "POST",
        assets_uri(&pid),
        &member,
        Some(json!({"kind":"note","title":"kickoff","metadata":{"body":"first notes"}})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let note = body(resp).await;
    assert_eq!(note["kind"], "note");
    assert_eq!(note["metadata"]["body"], "first notes");

    // Pointer kinds must carry uri or daas_artifact_id.
    for bad in [
        json!({"kind":"kg_slice"}),
        json!({"kind":"research_draft","uri":"  "}),
        json!({"kind":"bogus","uri":"urn:x"}),
        json!({"kind":"note","metadata":[1,2]}),
        json!({"kind":"note","metadata":"text"}),
    ] {
        let resp = call(
            app.clone(),
            "POST",
            assets_uri(&pid),
            &member,
            Some(bad.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad}");
    }

    let resp = call(
        app.clone(),
        "POST",
        assets_uri(&pid),
        &member,
        Some(json!({"kind":"kg_slice","daas_artifact_id":"art-1","title":"kg"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let kg = body(resp).await;
    assert_eq!(kg["daas_artifact_id"], "art-1");

    let resp = call(
        app.clone(),
        "POST",
        assets_uri(&pid),
        &viewer,
        Some(json!({"kind":"note"})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "viewer cannot write assets"
    );
    let resp = call(
        app.clone(),
        "POST",
        assets_uri(&pid),
        &outsider,
        Some(json!({"kind":"note"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = call(app.clone(), "GET", assets_uri(&pid), &outsider, None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // kind filter + get + soft delete.
    let resp = call(app.clone(), "GET", assets_uri(&pid), &viewer, None).await;
    assert_eq!(resp.status(), StatusCode::OK, "viewer lists assets");
    assert_eq!(body(resp).await["data"].as_array().unwrap().len(), 2);
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?kind=kg_slice", assets_uri(&pid)),
        &viewer,
        None,
    )
    .await;
    let filtered = body(resp).await;
    assert_eq!(filtered["data"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["data"][0]["kind"], "kg_slice");
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?kind=nope", assets_uri(&pid)),
        &viewer,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Keyset pagination walks the live set (note + kg_slice at this point).
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?limit=1", assets_uri(&pid)),
        &owner,
        None,
    )
    .await;
    let page1 = body(resp).await;
    assert_eq!(page1["data"].as_array().unwrap().len(), 1);
    let cursor = page1["next_cursor"]
        .as_str()
        .expect("second page")
        .replace('+', "%2B")
        .replace('|', "%7C");
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?limit=1&cursor={cursor}", assets_uri(&pid)),
        &owner,
        None,
    )
    .await;
    let page2 = body(resp).await;
    assert_eq!(page2["data"].as_array().unwrap().len(), 1);
    assert_ne!(page1["data"][0]["id"], page2["data"][0]["id"]);
    assert!(page2["next_cursor"].is_null());

    let asset_id = note["id"].as_str().unwrap();
    let resp = call(
        app.clone(),
        "GET",
        format!("{}/{asset_id}", assets_uri(&pid)),
        &viewer,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = call(
        app.clone(),
        "DELETE",
        format!("{}/{asset_id}", assets_uri(&pid)),
        &viewer,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "viewer cannot delete");
    let resp = call(
        app.clone(),
        "DELETE",
        format!("{}/{asset_id}", assets_uri(&pid)),
        &member,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = call(
        app.clone(),
        "GET",
        format!("{}/{asset_id}", assets_uri(&pid)),
        &owner,
        None,
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "soft-deleted reads 404"
    );

    // Deleted project is uniformly 404.
    let resp = call(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{pid}"),
        &owner,
        Some(json!({"status":"deleted"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = call(app.clone(), "GET", assets_uri(&pid), &owner, None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_requires_binding_and_persist_acl() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (viewer, _, viewer_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state.clone());
    let pid = create_project(app.clone(), &owner, "Search entry").await;
    add_member(app.clone(), &owner, &pid, &viewer_wid, "viewer").await;
    let search_uri = format!("/api/v1/research-projects/{pid}/search");

    let resp = call(
        app.clone(),
        "POST",
        search_uri.clone(),
        &owner,
        Some(json!({"query":"attention"})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "unbound project cannot search"
    );

    let resp = call(
        app.clone(),
        "PUT",
        binding_uri(&pid),
        &owner,
        Some(json!({"workspace_id": "ws-search"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    for bad in [
        json!({"query":"  "}),
        json!({}),
        json!({"query":"a\u{0007}"}),
    ] {
        let resp = call(
            app.clone(),
            "POST",
            search_uri.clone(),
            &owner,
            Some(bad.clone()),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad}");
    }

    let resp = call(
        app.clone(),
        "POST",
        search_uri.clone(),
        &viewer,
        Some(json!({"query":"attention","limit":5})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "viewer can search");
    let found = body(resp).await;
    assert_eq!(found["provider"], "stub");
    assert_eq!(found["workspace_id"], "ws-search");
    assert!(found["hits"].as_array().unwrap().is_empty());
    assert!(found["snapshot_asset_id"].is_null());

    // Persist is a write: viewer lacks Member + projects:write checks apply.
    let resp = call(
        app.clone(),
        "POST",
        search_uri.clone(),
        &viewer,
        Some(json!({"query":"attention","persist":true})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = call(
        app.clone(),
        "POST",
        search_uri.clone(),
        &owner,
        Some(json!({"query":"attention","persist":true})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let snapshot_id = body(resp).await["snapshot_asset_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let resp = call(
        app.clone(),
        "GET",
        format!("{}/{snapshot_id}", assets_uri(&pid)),
        &owner,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let snapshot = body(resp).await;
    assert_eq!(snapshot["kind"], "search_snapshot");
    assert_eq!(snapshot["metadata"]["query"], "attention");
}

#[tokio::test]
async fn import_pdf_stub_and_direct_pointer() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (viewer, _, viewer_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state.clone());
    let pid = create_project(app.clone(), &owner, "PDF intake").await;
    add_member(app.clone(), &owner, &pid, &viewer_wid, "viewer").await;
    let pdf_uri = format!("/api/v1/research-projects/{pid}/papers/from-pdf");

    // Direct pointer needs no binding or upstream call.
    let resp = call(
        app.clone(),
        "POST",
        pdf_uri.clone(),
        &owner,
        Some(json!({"daas_document_id":"doc-preexisting","title":"already parsed"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let direct = body(resp).await;
    assert_eq!(direct["daas_document_id"], "doc-preexisting");
    assert_eq!(direct["cached_title"], "already parsed");

    // Upstream registration requires an active binding.
    let resp = call(
        app.clone(),
        "POST",
        pdf_uri.clone(),
        &owner,
        Some(json!({"title":"paper.pdf"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    let resp = call(
        app.clone(),
        "PUT",
        binding_uri(&pid),
        &owner,
        Some(json!({"workspace_id": "ws-docs"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = call(
        app.clone(),
        "POST",
        pdf_uri.clone(),
        &viewer,
        Some(json!({"title":"viewer.pdf"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "viewer cannot import");

    let resp = call(
        app.clone(),
        "POST",
        pdf_uri.clone(),
        &owner,
        Some(json!({"title":"paper.pdf","fuel_role":"related","tags":["pdf"]})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "stub import must not block"
    );
    let imported = body(resp).await;
    assert!(
        imported["daas_document_id"]
            .as_str()
            .unwrap()
            .starts_with("stub-doc-"),
        "stub provider mints honest ids"
    );
    assert_eq!(imported["cached_title"], "paper.pdf");
    assert_eq!(imported["fuel_role"], "related");
}
