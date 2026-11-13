//! P1 project fuel-shelf integration tests. Requires DATABASE_URL.

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
    extract::Path,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::get,
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
            .join(format!("automl-paper-objects-{}", Uuid::new_v4()))
            .to_string_lossy()
            .into(),
        storage_public_base: "http://127.0.0.1:8030".into(),
        gitea: None,
        cli_dist_base: "https://dist.example.test".into(),
        public_base_url: "https://accept.example.test".into(),
        paper_enrichment: EnrichmentConfig::default(),
        daas: DaasConfig::default(),
    }
}

async fn setup() -> AppState {
    setup_with_enrichment(EnrichmentConfig::default()).await
}

async fn setup_with_enrichment(enrichment: EnrichmentConfig) -> AppState {
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
    let mut cfg = test_config();
    cfg.paper_enrichment = enrichment.clone();
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
        paper_enrichment: Arc::new(PaperEnrichment::new(&enrichment)),
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

async fn request(
    app: axum::Router,
    method: &str,
    uri: String,
    key: &str,
    raw: Option<String>,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {key}"));
    if raw.is_some() {
        req = req.header("content-type", "application/json");
    }
    app.oneshot(req.body(Body::from(raw.unwrap_or_default())).unwrap())
        .await
        .unwrap()
}

async fn call(
    app: axum::Router,
    method: &str,
    uri: String,
    key: &str,
    value: Option<Value>,
) -> axum::response::Response {
    request(app, method, uri, key, value.map(|v| v.to_string())).await
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

fn papers_uri(project: &str) -> String {
    format!("/api/v1/research-projects/{project}/papers")
}

#[tokio::test]
async fn discriminator_validation_and_stub_identifiers() {
    let state = setup().await;
    let (key, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state);
    let pid = create_project(app.clone(), &key, "Fuel shelf identifiers").await;

    for invalid in [
        json!({}),
        json!({"doi":"10.1000/a", "arxiv_id":"2401.12345"}),
        json!({"doi": 7}),
        json!({"doi": "not-a-doi"}),
        json!({"other":"x"}),
        json!({"doi":"  "}),
        json!({"arxiv_id":""}),
        json!({"w_papers_work_id":"\t"}),
    ] {
        let resp = call(app.clone(), "POST", papers_uri(&pid), &key, Some(invalid)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    let cases = [
        (json!({"doi":"10.1000/ABC"}), "doi", "10.1000/abc"),
        (json!({"arxiv_id":"2401.12345"}), "arxiv_id", "2401.12345"),
        (
            json!({"w_papers_work_id":"wp-work-7"}),
            "w_papers_work_id",
            "wp-work-7",
        ),
        (json!({"openalex_id":"W123"}), "openalex_id", "W123"),
        (
            json!({"daas_document_id":"daas-9"}),
            "daas_document_id",
            "daas-9",
        ),
    ];
    for (input, field, expected) in cases {
        let resp = call(app.clone(), "POST", papers_uri(&pid), &key, Some(input)).await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let paper = body(resp).await;
        assert_eq!(paper[field], expected);
        assert_eq!(paper["fuel_role"], "seed");
        assert!(
            paper["cached_title"].is_null(),
            "stub must not invent metadata"
        );
    }
    let resp = call(app.clone(), "GET", papers_uri(&pid), &key, None).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body(resp).await["data"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn normalized_doi_conflicts_are_project_local_and_audited() {
    let state = setup().await;
    let (key, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state.clone());
    let first = create_project(app.clone(), &key, "First shelf").await;
    let second = create_project(app.clone(), &key, "Second shelf").await;

    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&first),
        &key,
        Some(
            json!({"doi":" https://doi.org/10.5555/AbC ","fuel_role":"baseline","notes":"origin","tags":["graph"]}),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let first_paper = body(resp).await;
    assert_eq!(first_paper["fuel_role"], "baseline");
    assert_eq!(first_paper["notes"], "origin");
    assert_eq!(first_paper["tags"], json!(["graph"]));

    // DOI resolver prefix, surrounding whitespace, and case normalize to one key.
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&first),
        &key,
        Some(json!({"doi":"10.5555/abc"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&second),
        &key,
        Some(json!({"doi":"10.5555/abc"})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "DOI uniqueness is scoped to project"
    );

    let pid = Uuid::parse_str(&first).unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE project_id=$1 AND action='paper.added'",
    )
    .bind(pid)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(
        count, 1,
        "successful create has one audit row; rejected duplicate has none"
    );
}

#[tokio::test]
async fn paper_shelf_acl_visibility_soft_delete_and_keyset() {
    let state = setup().await;
    let (owner, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (member, _, member_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (viewer, _, viewer_wid) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (outsider, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let (read_only, _, _) = mint_key(&state, &[scope::PROJECTS_READ]).await;
    let app = build_router(state.clone());
    let pid = create_project(app.clone(), &owner, "Paper shelf access").await;

    for (wid, role) in [(&member_wid, "member"), (&viewer_wid, "viewer")] {
        let resp = call(
            app.clone(),
            "POST",
            format!("/api/v1/research-projects/{pid}/members"),
            &owner,
            Some(json!({"scholar":wid,"role":role})),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&pid),
        &owner,
        Some(json!({"doi":"10.7777/equal-a"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&pid),
        &member,
        Some(json!({"arxiv_id":"2402.00002"})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "project member can add fuel"
    );
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&pid),
        &viewer,
        Some(json!({"doi":"10.7777/viewer-denied"})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "viewer cannot create fuel"
    );
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&pid),
        &read_only,
        Some(json!({"doi":"10.7777/no-write-scope"})),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "missing projects:write is denied"
    );

    let resp = call(app.clone(), "GET", papers_uri(&pid), &outsider, None).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let resp = call(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{pid}"),
        &owner,
        Some(json!({"visibility":"public_meta"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = call(app.clone(), "GET", papers_uri(&pid), &outsider, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "public_meta project metadata does not expose shelf contents"
    );

    // Force a timestamp tie so the UUID tie-breaker is exercised.
    let project_id = Uuid::parse_str(&pid).unwrap();
    sqlx::query("UPDATE project_papers SET created_at='2026-01-01T00:00:00Z' WHERE project_id=$1")
        .bind(project_id)
        .execute(&state.db)
        .await
        .unwrap();
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?limit=1", papers_uri(&pid)),
        &owner,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let page1 = body(resp).await;
    let cursor = page1["next_cursor"]
        .as_str()
        .expect("second page available")
        .replace('+', "%2B")
        .replace('|', "%7C");
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?limit=1&cursor={cursor}", papers_uri(&pid)),
        &owner,
        None,
    )
    .await;
    let page2 = body(resp).await;
    assert_eq!(page2["data"].as_array().unwrap().len(), 1);
    assert_ne!(page1["data"][0]["id"], page2["data"][0]["id"]);
    assert!(page2["next_cursor"].is_null());
    let resp = call(
        app.clone(),
        "GET",
        format!("{}?cursor=not-a-cursor", papers_uri(&pid)),
        &owner,
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = call(
        app.clone(),
        "PATCH",
        format!("/api/v1/research-projects/{pid}"),
        &owner,
        Some(json!({"status":"deleted"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = call(app.clone(), "GET", papers_uri(&pid), &owner, None).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

async fn metadata_fixture(Path(path): Path<String>) -> impl IntoResponse {
    if path.contains("fail") {
        return (StatusCode::INTERNAL_SERVER_ERROR, "upstream unavailable").into_response();
    }
    if path.contains("malformed") {
        return (StatusCode::OK, "not-json").into_response();
    }
    (
        StatusCode::OK,
        axum::Json(json!({
            "display_name": "Cached from fixture",
            "title": "Cached from fixture",
            "publication_year": 2024,
            "year": 2024,
            "id": "https://openalex.org/W987654321",
            "doi": "https://doi.org/10.9000/cache",
            "authorships": [{"author":{"display_name":"A. Researcher"}}],
            "authors": ["A. Researcher"]
        })),
    )
        .into_response()
}

#[tokio::test]
async fn best_effort_w_papers_failures_and_daas_metadata_cache() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fixture = axum::Router::new().route("/{*path}", get(metadata_fixture));
    tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });

    let enrichment = EnrichmentConfig {
        openalex_mode: None,
        daas_openalex_base: Some(base.clone()),
        w_papers_base: Some(base),
    };
    let state = setup_with_enrichment(enrichment).await;
    let (key, _, _) = mint_key(&state, ALL_PROJECT_SCOPES).await;
    let app = build_router(state);
    let pid = create_project(app.clone(), &key, "Enrichment fixture").await;

    for wid in ["fail", "malformed"] {
        let resp = call(
            app.clone(),
            "POST",
            papers_uri(&pid),
            &key,
            Some(json!({"w_papers_work_id":wid})),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "upstream {wid} must not block shelf insertion"
        );
        let paper = body(resp).await;
        assert_eq!(paper["w_papers_work_id"], wid);
        assert!(paper["cached_title"].is_null());
    }

    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&pid),
        &key,
        Some(json!({"w_papers_work_id":"ok"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(body(resp).await["cached_title"], "Cached from fixture");

    // DaaS OpenAlex-shaped result can enrich a DOI row with cache data and a
    // secondary OpenAlex pointer while retaining the submitted DOI.
    let resp = call(
        app.clone(),
        "POST",
        papers_uri(&pid),
        &key,
        Some(json!({"doi":"10.9000/cache"})),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let paper = body(resp).await;
    assert_eq!(paper["doi"], "10.9000/cache");
    assert_eq!(paper["openalex_id"], "https://openalex.org/W987654321");
    assert_eq!(paper["cached_title"], "Cached from fixture");
    assert_eq!(paper["cached_year"], 2024);
    assert_eq!(paper["cached_authors"], json!(["A. Researcher"]));
}
