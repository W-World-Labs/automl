//! Integration tests for Research Hub P0 (research projects).
//! Requires DATABASE_URL (same as papers_integration).

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
            .join(format!("automl-proj-objects-{}", Uuid::new_v4()))
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

/// Fresh scholar + API key with the given scopes. Returns (key, scholar_id, w_id).
async fn mint_key(state: &AppState, scopes: &[&str]) -> (String, Uuid, String) {
    let w_id = format!("s-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let scholar = sso::ensure_scholar(&state.db, &w_id, &w_id).await.unwrap();
    let scopes_vec: Vec<String> = scopes.iter().map(|s| (*s).to_string()).collect();
    let created = keys::create_key(
        &state.db,
        scholar,
        CreateKeyRequest {
            name: format!("k-{w_id}"),
            scopes: scopes_vec.clone(),
            expires_at: None,
        },
        &scopes_vec,
        true,
    )
    .await
    .unwrap();
    (created.key, scholar, w_id)
}

const PROJECT_SCOPES: &[&str] = &[
    scope::PROJECTS_READ,
    scope::PROJECTS_WRITE,
    scope::PROJECTS_ADMIN,
];

async fn post_project(app: axum::Router, key: &str, title: &str) -> Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/research-projects")
                .header("authorization", format!("Bearer {key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": title}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    json_body(resp).await
}

async fn authed(
    app: axum::Router,
    method: &str,
    uri: String,
    key: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {key}"));
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    app.oneshot(
        req.body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn project_crud_and_status_machine() {
    let state = setup().await;
    let (key, _sid, _wid) = mint_key(&state, PROJECT_SCOPES).await;
    let app = build_router(state);

    let p = post_project(app.clone(), &key, "Graph Kernels").await;
    let id = p["id"].as_str().unwrap().to_string();
    assert_eq!(p["status"], "draft");
    assert_eq!(p["visibility"], "private");
    assert_eq!(p["slug"], "graph-kernels"); // derived from title

    // GET + list as owner.
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{id}"),
        &key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Illegal: draft → archived (state machine → 409).
    let resp = authed(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{id}"),
        &key,
        Some(json!({"status": "archived"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(resp).await["error"]["code"], "CONFLICT");

    // Field edit (owner, projects:write).
    let resp = authed(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{id}"),
        &key,
        Some(json!({"title": "Graph Kernels v2", "summary": "s"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(json_body(resp).await["title"], "Graph Kernels v2");

    // Legal lifecycle: draft → active → archived → deleted.
    for to in ["active", "archived", "deleted"] {
        let resp = authed(
            app.clone(),
            "PATCH",
            format!("/api/v1/research-projects/{id}"),
            &key,
            Some(json!({"status": to})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "→ {to}");
        assert_eq!(json_body(resp).await["status"], to);
    }

    // Soft-deleted projects are invisible.
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{id}"),
        &key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // And the slug is freed for reuse.
    let p2 = post_project(app.clone(), &key, "Graph Kernels").await;
    assert_eq!(p2["slug"], "graph-kernels");
}

#[tokio::test]
async fn non_member_and_scope_gates() {
    let state = setup().await;
    let (owner_key, _s, _w) = mint_key(&state, PROJECT_SCOPES).await;
    let (outsider_key, _s2, _w2) = mint_key(&state, PROJECT_SCOPES).await;
    // Holds the project roles' scope but only papers-level scopes… actually
    // mint a key with NO project scopes for the scope-gate check.
    let (no_scope_key, _s3, _w3) = mint_key(&state, &[scope::PAPERS_READ]).await;
    let app = build_router(state);

    let p = post_project(app.clone(), &owner_key, "Secret Lab").await;
    let id = p["id"].as_str().unwrap().to_string();

    // Missing projects:* scope → 403 even before membership is considered.
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{id}"),
        &no_scope_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = authed(
        app.clone(),
        "POST",
        "/api/v1/research-projects".into(),
        &no_scope_key,
        Some(json!({"title": "x"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Non-member with scopes → 403 everywhere on the private project.
    for (method, uri) in [
        ("GET", format!("/api/v1/research-projects/{id}")),
        ("PATCH", format!("/api/v1/research-projects/{id}")),
        ("GET", format!("/api/v1/research-projects/{id}/members")),
        ("GET", format!("/api/v1/research-projects/{id}/audit")),
        ("GET", format!("/api/v1/research-projects/{id}/papers")),
    ] {
        let resp = authed(
            app.clone(),
            method,
            uri.clone(),
            &outsider_key,
            if method == "PATCH" {
                Some(json!({"title": "x"}))
            } else {
                None
            },
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{method} {uri}");
    }

    // public_meta flips read access for any authenticated principal.
    let resp = authed(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{id}"),
        &owner_key,
        Some(json!({"visibility": "public_meta"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{id}"),
        &outsider_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    // …but member/audit subroutes still require membership.
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{id}/members"),
        &outsider_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn members_add_list_remove_and_guards() {
    let state = setup().await;
    // reproduce:submit is what POST /api/v1/agents requires (agent creation).
    let mut owner_scopes = PROJECT_SCOPES.to_vec();
    owner_scopes.push(scope::REPRODUCE_SUBMIT);
    let (owner_key, _os, _ow) = mint_key(&state, &owner_scopes).await;
    let (member_key, _ms, mw) = mint_key(&state, PROJECT_SCOPES).await;
    let app = build_router(state.clone());

    let p = post_project(app.clone(), &owner_key, "Team Project").await;
    let pid = p["id"].as_str().unwrap().to_string();

    // Owner adds a scholar member by w_id.
    let resp = authed(
        app.clone(),
        "POST",
        format!("/api/v1/research-projects/{pid}/members"),
        &owner_key,
        Some(json!({"scholar": mw, "role": "member"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let member = json_body(resp).await;
    let member_row_id = member["id"].as_str().unwrap().to_string();
    assert_eq!(member["scholar_w_id"], mw);

    // Duplicate → 409.
    let resp = authed(
        app.clone(),
        "POST",
        format!("/api/v1/research-projects/{pid}/members"),
        &owner_key,
        Some(json!({"scholar": mw, "role": "member"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Member list has 2 rows; the new member can now read.
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}/members"),
        &owner_key,
        None,
    )
    .await;
    assert_eq!(json_body(resp).await["data"].as_array().unwrap().len(), 2);
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}"),
        &member_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // …but a Member cannot patch the project or touch the ACL (owner work).
    let resp = authed(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{pid}"),
        &member_key,
        Some(json!({"title": "hijack"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = authed(
        app.clone(),
        "POST",
        format!("/api/v1/research-projects/{pid}/members"),
        &member_key,
        Some(json!({"scholar": "nobody", "role": "viewer"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Creating an agent does not establish request identity. P0 explicitly
    // defers agent enrollment for every role, including viewer/member.
    let resp = authed(
        app.clone(),
        "POST",
        "/api/v1/agents".into(),
        &owner_key,
        Some(json!({"name": "helper-bot"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let agent_id = json_body(resp).await["id"].as_str().unwrap().to_string();
    for role in ["owner", "member", "viewer"] {
        let resp = authed(
            app.clone(),
            "POST",
            format!("/api/v1/research-projects/{pid}/members"),
            &owner_key,
            Some(json!({"agent": agent_id, "role": role})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
        assert_eq!(json_body(resp).await["error"]["code"], "NOT_IMPLEMENTED");
    }
    let agent_members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_members WHERE project_id = $1 AND principal_type = 'agent'",
    )
    .bind(Uuid::parse_str(&pid).unwrap())
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(agent_members, 0);

    // Removing the last owner is rejected.
    let owner_member_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM project_members WHERE project_id = $1 AND role = 'owner'",
    )
    .bind(Uuid::parse_str(&pid).unwrap())
    .fetch_one(&state.db)
    .await
    .unwrap();
    let resp = authed(
        app.clone(),
        "DELETE",
        format!("/api/v1/research-projects/{pid}/members/{owner_member_id}"),
        &owner_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Owner removes the added member; their access ends immediately.
    let resp = authed(
        app.clone(),
        "DELETE",
        format!("/api/v1/research-projects/{pid}/members/{member_row_id}"),
        &owner_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}"),
        &member_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn keyset_pagination() {
    let state = setup().await;
    let (key, _s, _w) = mint_key(&state, PROJECT_SCOPES).await;
    let app = build_router(state);

    for i in 0..3 {
        post_project(app.clone(), &key, &format!("Paginated {i}")).await;
    }

    let resp = authed(
        app.clone(),
        "GET",
        "/api/v1/research-projects?limit=2".into(),
        &key,
        None,
    )
    .await;
    let page1 = json_body(resp).await;
    assert_eq!(page1["data"].as_array().unwrap().len(), 2);
    let cursor = page1["next_cursor"].as_str().expect("more pages");

    // The cursor is `<rfc3339>|<uuid>` — `+` in the offset must be
    // percent-encoded or it decodes as a space.
    let cursor = cursor.replace('+', "%2B").replace('|', "%7C");
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects?limit=2&cursor={cursor}"),
        &key,
        None,
    )
    .await;
    let page2 = json_body(resp).await;
    assert_eq!(page2["data"].as_array().unwrap().len(), 1);
    assert!(page2["next_cursor"].is_null());

    let ids1: Vec<&str> = page1["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    let ids2: Vec<&str> = page2["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    for id in &ids2 {
        assert!(!ids1.contains(id), "pages must not overlap");
    }
}

#[tokio::test]
async fn audit_trail_and_empty_shelf() {
    let state = setup().await;
    let (owner_key, _s, _w) = mint_key(&state, PROJECT_SCOPES).await;
    let (_k2, _s2, member_w) = mint_key(&state, PROJECT_SCOPES).await;
    let app = build_router(state);

    let p = post_project(app.clone(), &owner_key, "Audit Me").await;
    let pid = p["id"].as_str().unwrap().to_string();
    authed(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{pid}"),
        &owner_key,
        Some(json!({"status": "active"})),
    )
    .await;
    authed(
        app.clone(),
        "POST",
        format!("/api/v1/research-projects/{pid}/members"),
        &owner_key,
        Some(json!({"scholar": member_w, "role": "viewer"})),
    )
    .await;

    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}/audit"),
        &owner_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let audit = json_body(resp).await;
    let actions: Vec<&str> = audit["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for expected in ["project.created", "project.status_changed", "member.added"] {
        assert!(
            actions.contains(&expected),
            "missing audit action {expected}"
        );
    }

    // P0 shelf: honest empty page.
    let resp = authed(
        app.clone(),
        "GET",
        format!("/api/v1/research-projects/{pid}/papers"),
        &owner_key,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let shelf = json_body(resp).await;
    assert_eq!(shelf["data"].as_array().unwrap().len(), 0);
    assert!(shelf["next_cursor"].is_null());
}

#[tokio::test]
async fn visibility_matrix_and_public_metadata_allowlist() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, PROJECT_SCOPES).await;
    let (viewer, _, viewer_wid) = mint_key(&state, PROJECT_SCOPES).await;
    let (outsider, _, _) = mint_key(&state, PROJECT_SCOPES).await;
    let app = build_router(state);
    let lab = Uuid::new_v4();
    let resp = authed(
        app.clone(),
        "POST",
        "/api/v1/research-projects".into(),
        &owner,
        Some(
            json!({"title":"Visibility fixture", "summary":"Public summary", "lab_id":lab,
                    "w_daas_workspace_id":"unverified-private-pointer"}),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let project = json_body(resp).await;
    let id = project["id"].as_str().unwrap();
    let uri = format!("/api/v1/research-projects/{id}");
    let resp = authed(
        app.clone(),
        "POST",
        format!("{uri}/members"),
        &owner,
        Some(json!({"scholar":viewer_wid,"role":"viewer"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    for visibility in ["private", "lab", "public_meta"] {
        let resp = authed(
            app.clone(),
            "PATCH",
            uri.clone(),
            &owner,
            Some(json!({"visibility":visibility})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        for member_key in [&owner, &viewer] {
            let resp = authed(app.clone(), "GET", uri.clone(), member_key, None).await;
            assert_eq!(resp.status(), StatusCode::OK);
            let full = json_body(resp).await;
            assert_eq!(full["created_by"], project["created_by"]);
            assert_eq!(full["lab_id"], lab.to_string());
            assert_eq!(full["w_daas_workspace_id"], "unverified-private-pointer");
        }
        let resp = authed(app.clone(), "GET", uri.clone(), &outsider, None).await;
        if visibility == "public_meta" {
            assert_eq!(resp.status(), StatusCode::OK);
            let public = json_body(resp).await;
            let keys: std::collections::BTreeSet<_> = public
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                [
                    "id",
                    "slug",
                    "title",
                    "summary",
                    "visibility",
                    "status",
                    "created_at",
                    "updated_at"
                ]
                .into_iter()
                .collect()
            );
            assert_eq!(public["summary"], "Public summary");
        } else {
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        }
        for subroute in ["papers", "members", "audit"] {
            let resp = authed(
                app.clone(),
                "GET",
                format!("{uri}/{subroute}"),
                &outsider,
                None,
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "{visibility}/{subroute}"
            );
            let resp = authed(
                app.clone(),
                "GET",
                format!("{uri}/{subroute}"),
                &viewer,
                None,
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "viewer {visibility}/{subroute}"
            );
        }
    }
    // A matching inert lab pointer never grants access or project discovery.
    let resp = authed(
        app.clone(),
        "POST",
        "/api/v1/research-projects".into(),
        &outsider,
        Some(json!({"title":"Same lab pointer", "lab_id":lab})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = authed(
        app.clone(),
        "GET",
        "/api/v1/research-projects".into(),
        &outsider,
        None,
    )
    .await;
    let listing = json_body(resp).await;
    assert!(listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["id"] != id));
    let resp = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn scholar_delegated_keys_keep_scope_and_membership_boundaries() {
    let state = setup().await;
    let (owner_key, owner_id, _) = mint_key(&state, PROJECT_SCOPES).await;
    let (outsider_key, _, outsider_wid) = mint_key(&state, PROJECT_SCOPES).await;
    let scopes = vec![
        scope::PROJECTS_READ.to_owned(),
        scope::PROJECTS_WRITE.to_owned(),
    ];
    let limited = keys::create_key(
        &state.db,
        owner_id,
        CreateKeyRequest {
            name: "title editor".into(),
            scopes: scopes.clone(),
            expires_at: None,
        },
        &scopes,
        true,
    )
    .await
    .unwrap();
    let app = build_router(state.clone());
    let p = post_project(app.clone(), &owner_key, "Delegated identity").await;
    let pid = p["id"].as_str().unwrap();
    let uri = format!("/api/v1/research-projects/{pid}");
    let resp = authed(
        app.clone(),
        "PATCH",
        uri.clone(),
        &limited.key,
        Some(json!({"title":"Allowed title edit"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    for patch in [
        json!({"status":"active"}),
        json!({"visibility":"public_meta"}),
    ] {
        let resp = authed(app.clone(), "PATCH", uri.clone(), &limited.key, Some(patch)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
    let resp = authed(
        app.clone(),
        "POST",
        format!("{uri}/members"),
        &limited.key,
        Some(json!({"scholar":outsider_wid,"role":"member"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // Scopes alone cannot enroll scholars or probe deferred agent enrollment.
    for member in [
        json!({"scholar":outsider_wid,"role":"owner"}),
        json!({"agent":Uuid::new_v4(),"role":"viewer"}),
    ] {
        let resp = authed(
            app.clone(),
            "POST",
            format!("{uri}/members"),
            &outsider_key,
            Some(member),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
    // The human CLI path remains supported with an owner's admin-scoped key.
    let resp = authed(
        app.clone(),
        "POST",
        format!("{uri}/members"),
        &owner_key,
        Some(json!({"scholar":outsider_wid,"role":"member"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    keys::revoke_key(&state.db, owner_id, limited.id)
        .await
        .unwrap();
    let resp = authed(app, "GET", uri, &limited.key, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
