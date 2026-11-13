use std::{sync::Arc, time::Instant};

use w_id_sdk::SsoConfig;
use w_storage::Storage;

use crate::{
    config::Config, daas::Daas, domain::gitea::GiteaClient, paper_enrichment::PaperEnrichment,
};

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub cfg: Arc<Config>,
    pub started_at: Instant,
    /// `None` when W-ID env is missing — SSO routes return 503, API keys still work.
    pub sso: Option<SsoConfig>,
    /// Object storage (local or S3). Always present; assets require it.
    pub storage: Arc<dyn Storage>,
    /// Dedicated Gitea integration. `None` when `GITEA_*` env is absent —
    /// webhook endpoint answers 501, papers require an external repo_url.
    pub gitea: Option<Arc<GiteaClient>>,
    /// Best-effort metadata provider; defaults to a local stub.
    pub paper_enrichment: Arc<PaperEnrichment>,
    /// W-DaaS provider for project bindings/search/document import;
    /// `StubDaaS` unless `DAAS_API_BASE` is configured.
    pub daas: Arc<Daas>,
}
