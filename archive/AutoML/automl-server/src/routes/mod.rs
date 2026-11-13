// ============================================================
// Routes — API router registration
// ============================================================

pub mod assets;
pub mod community;
pub mod compute;
pub mod papers;
pub mod research;
pub mod reviews;
pub mod scholars;
pub mod sessions;
pub mod silicon_scholar;
pub mod system;
pub mod templates;

use crate::config::Config;
use crate::db::Pool;
use crate::storage::Storage;
use axum::Router;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Pool,
    pub storage: Storage,
    pub config: Config,
    pub sso: Option<w_id_sdk::SsoConfig>,
    pub radar_configured: bool,
    pub started_at: std::time::Instant,
}

/// Build the full API v1 router
pub fn api_router() -> Router<AppState> {
    Router::new()
        .merge(papers::router())
        .merge(scholars::router())
        .merge(silicon_scholar::router())
        .merge(assets::router())
        .merge(reviews::router())
        .merge(research::router())
        .merge(community::router())
        .merge(compute::router())
        .merge(templates::router())
        .merge(system::router())
}

/// Build root-level routes that are not versioned API endpoints.
pub fn root_router() -> Router<AppState> {
    Router::new()
        .merge(sessions::router())
        .merge(system::root_router())
}
