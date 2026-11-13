//! SEO/GEO surface: anonymous public reads, robots/sitemap/llms, and
//! server-rendered paper head metadata. Requires DATABASE_URL.

use std::sync::Arc;

use automl_server::{
    auth::sso,
    build_router,
    config::{Config, RateLimitConfig},
    domain::papers,
    state::AppState,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "https://accept.example.test";

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
            .join(format!("automl-seo-objects-{}", Uuid::new_v4()))
            .to_string_lossy()
            .into(),
        storage_public_base: "http://127.0.0.1:8030".into(),
        gitea: None,
        paper_enrichment: automl_server::config::EnrichmentConfig::default(),
        daas: automl_server::config::DaasConfig::default(),
        cli_dist_base: "https://dist.example.test".into(),
        public_base_url: BASE.into(),
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
        sso: None,
        storage,
        gitea: None,
        paper_enrichment: Arc::new(automl_server::paper_enrichment::PaperEnrichment::stub()),
        daas: Arc::new(automl_server::daas::Daas::stub()),
    }
}

async fn text_body(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).to_string()
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let resp = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    (status, text_body(resp).await)
}

async fn seed_paper(state: &AppState, title: &str, accept: bool) -> Uuid {
    let owner = sso::ensure_scholar(&state.db, &format!("seo-{}", Uuid::new_v4()), "s")
        .await
        .unwrap();
    let paper = papers::create(
        &state.db,
        owner,
        serde_json::from_value(json!({
            "title": title,
            "abstract": "We study reproducibility at scale across heterogeneous environments.",
            "authors": ["Ada Lovelace"],
            "repo_url": "https://code.example.test/x.git",
            "commit_sha": COMMIT,
        }))
        .unwrap(),
        false,
    )
    .await
    .unwrap();
    if accept {
        for s in ["submitted", "under_review", "accepted"] {
            papers::patch(
                &state.db,
                paper.id,
                serde_json::from_value(json!({"review_status": s})).unwrap(),
            )
            .await
            .unwrap();
        }
    }
    paper.id
}

#[tokio::test]
async fn public_reads_need_no_auth() {
    let state = setup().await;
    let id = seed_paper(&state, "Public read target", true).await;
    let app = build_router(state);

    // Visitor role (PRODUCT-FLOWS): papers, reviews, reproduction, forum reads.
    for uri in [
        "/api/v1/papers?limit=1".to_string(),
        format!("/api/v1/papers/{id}"),
        format!("/api/v1/papers/{id}/reviews"),
        format!("/api/v1/papers/{id}/reproduction"),
        format!("/api/v1/reproduce/runs?paper_id={id}"),
        "/api/v1/forum/categories?limit=5".to_string(),
        "/api/v1/forum/threads?limit=5".to_string(),
    ] {
        let (status, body) = get(&app, &uri).await;
        assert_eq!(status, StatusCode::OK, "anonymous GET {uri}: {body}");
    }

    // Writes stay authed.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/papers")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": "nope"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "writes still need auth"
    );
}

#[tokio::test]
async fn robots_sitemap_llms() {
    let state = setup().await;
    let accepted = seed_paper(&state, "Sitemap member", true).await;
    let draft = seed_paper(&state, "Draft stays out", false).await;
    let app = build_router(state);

    let (status, robots) = get(&app, "/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(robots.contains("Disallow: /api/"));
    assert!(robots.contains(&format!("Sitemap: {BASE}/sitemap.xml")));

    let (status, sitemap) = get(&app, "/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        sitemap.contains(&format!("{BASE}/papers/{accepted}")),
        "accepted paper listed"
    );
    assert!(!sitemap.contains(&draft.to_string()), "draft not listed");
    assert!(sitemap.contains("<lastmod>"));

    let (status, llms) = get(&app, "/llms.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert!(llms.contains("verified-by-N"));
    assert!(llms.contains("openapi.json"));
}

#[tokio::test]
async fn paper_page_head_is_server_rendered() {
    let state = setup().await;
    let accepted = seed_paper(&state, "SEO & <GEO> paper", true).await;
    let draft = seed_paper(&state, "Draft paper", false).await;
    let app = build_router(state);

    // NOTE: web/dist may be empty in dev checkouts; the shell test only runs
    // when an index.html is embedded. The unit tests in seo.rs cover injection
    // itself; this asserts the routing + escaping end to end when possible.
    let (status, html) = get(&app, &format!("/papers/{accepted}")).await;
    if status == StatusCode::OK {
        assert!(html.contains("ScholarlyArticle"), "JSON-LD present");
        assert!(html.contains("BreadcrumbList"), "breadcrumbs present");
        assert!(
            html.contains("SEO &amp; &lt;GEO&gt; paper"),
            "escaped title"
        );
        assert!(html.contains("rel=\"canonical\""));
        assert!(!html.contains("name=\"robots\" content=\"noindex\""));
        // Crawler-visible body snapshot (not just an empty #root).
        assert!(
            html.contains("We study reproducibility at scale"),
            "abstract visible without JS"
        );
        assert!(html.contains("verified-by-"), "trust aggregate visible");
        assert!(html.contains("og:image"), "social image present");

        let (_, draft_html) = get(&app, &format!("/papers/{draft}")).await;
        assert!(draft_html.contains("noindex"), "draft pages are noindex");
    } else {
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "no embedded frontend → honest 404"
        );
    }

    // Unknown paper id falls through to the SPA shell (client shows its 404).
    let (fallback_status, _) = get(&app, &format!("/papers/{}", Uuid::new_v4())).await;
    assert!(
        fallback_status == StatusCode::OK || fallback_status == StatusCode::NOT_FOUND,
        "unknown id falls back to shell"
    );
}

#[tokio::test]
async fn index_and_thread_snapshots() {
    let state = setup().await;
    let accepted = seed_paper(&state, "Index snapshot paper", true).await;
    let app = build_router(state.clone());

    // Home + /papers carry a crawlable index of accepted papers.
    for path in ["/", "/papers"] {
        let (status, html) = get(&app, path).await;
        if status != StatusCode::OK {
            continue; // dev checkout without embedded frontend
        }
        assert!(
            html.contains(&format!("/papers/{accepted}")),
            "{path} links accepted papers for crawlers"
        );
        assert!(html.contains("Recent accepted papers"));
    }

    // Forum thread page: head + DiscussionForumPosting + visible posts.
    let owner = sso::ensure_scholar(&state.db, &format!("seo-t-{}", Uuid::new_v4()), "t")
        .await
        .unwrap();
    let (cat,): (Uuid,) = sqlx::query_as("SELECT id FROM forum_categories LIMIT 1")
        .fetch_one(&state.db)
        .await
        .unwrap();
    let thread = automl_server::domain::forum::create_thread(
        &state.db,
        owner,
        serde_json::from_value(json!({
            "category_id": cat,
            "title": "Snapshot thread <tag>",
            "body": "First post body for crawlers.",
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    let (status, html) = get(&app, &format!("/forum/threads/{}", thread.id)).await;
    if status == StatusCode::OK {
        assert!(html.contains("DiscussionForumPosting"));
        assert!(
            html.contains("Snapshot thread &lt;tag&gt;"),
            "escaped title"
        );
        assert!(html.contains("First post body for crawlers."));
        assert!(html.contains("rel=\"canonical\""));
    } else {
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    // Sitemap includes the thread.
    let (_, sitemap) = get(&app, "/sitemap.xml").await;
    assert!(
        sitemap.contains(&format!("/forum/threads/{}", thread.id)),
        "threads in sitemap"
    );
}
