// ============================================================
// spa — Embedded SPA (automl-web/dist) served via the shared
// w-web-kit `spa` module. The RustEmbed struct + #[folder] stay
// local (the folder path is manifest-relative); the handler just
// delegates to w_web_kit::spa_response so the guard/shell logic
// lives in one place across the fleet.
// ============================================================

use std::sync::LazyLock;

use axum::{http::Uri, response::Response};
use rust_embed::RustEmbed;
use w_web_kit::{spa_response, SpaConfig};

#[derive(RustEmbed)]
#[folder = "../automl-web/dist"]
struct SpaAssets;

// Guard list maps the ORIGINAL spa_handler `starts_with` checks faithfully:
//   starts_with("api/")    -> "api/"    (prefix-only; exact /api falls through)
//   starts_with("auth/")   -> "auth/"   (prefix-only)
//   starts_with("health")  -> "health"  (bare = exact /health AND /health/...)
//   starts_with("metrics") -> "metrics" (bare = exact /metrics AND /metrics/...)
//   starts_with("skills/") -> "skills/" (prefix-only)
// Shell stays index.html; no strict-asset 404 / Cache-Control hardening.
static SPA: LazyLock<SpaConfig> =
    LazyLock::new(|| SpaConfig::fleet_api_plus(["skills/"]));

/// Serve an embedded static asset by path, or fall back to index.html for SPA
/// client-side routes.  Used as the axum router `.fallback`, so it only
/// handles requests not matched by an explicit API/auth/health route.
pub async fn spa_handler(uri: Uri) -> Response {
    spa_response::<SpaAssets>(&uri, &SPA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, Uri};

    async fn status_and_ct(uri_str: &str) -> (u16, String) {
        let uri: Uri = uri_str.parse().unwrap();
        let response = spa_handler(uri).await;
        let status = response.status().as_u16();
        let ct = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap_or("").to_string())
            .unwrap_or_default();
        (status, ct)
    }

    #[tokio::test]
    async fn root_returns_html() {
        let (status, ct) = status_and_ct("/").await;
        assert_eq!(status, 200, "root should return 200");
        assert!(
            ct.contains("text/html"),
            "root content-type should be text/html, got: {ct}"
        );
    }

    #[tokio::test]
    async fn known_css_asset_returns_css() {
        let Some(css_path) = SpaAssets::iter().find(|p| p.ends_with(".css")) else {
            return; // no frontend build present (CI)
        };
        let uri_str = format!("/assets/{}", css_path.trim_start_matches("assets/"));
        let (status, ct) = status_and_ct(&uri_str).await;
        assert_eq!(status, 200, "css asset should return 200 for {uri_str}");
        assert!(
            ct.contains("text/css"),
            "css asset content-type should be text/css, got: {ct}"
        );
    }

    #[tokio::test]
    async fn api_path_returns_404() {
        let (status, _ct) = status_and_ct("/api/anything").await;
        assert_eq!(status, 404, "api path should return 404 from spa_handler");
    }

    #[tokio::test]
    async fn auth_path_returns_404() {
        let (status, _ct) = status_and_ct("/auth/login").await;
        assert_eq!(status, 404, "auth path should return 404 from spa_handler");
    }

    #[tokio::test]
    async fn metrics_path_returns_404() {
        let (status, _ct) = status_and_ct("/metrics").await;
        assert_eq!(
            status, 404,
            "metrics path should return 404 from spa_handler"
        );
    }

    #[tokio::test]
    async fn unknown_spa_route_returns_index_html() {
        let (status, ct) = status_and_ct("/some/client/route").await;
        assert_eq!(
            status, 200,
            "unknown SPA route should fall back to index.html"
        );
        assert!(
            ct.contains("text/html"),
            "SPA fallback should be text/html, got: {ct}"
        );
    }
}
