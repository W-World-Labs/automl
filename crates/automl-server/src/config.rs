use std::net::SocketAddr;

/// Runtime configuration, read once at startup from the environment
/// (`.env` is loaded by `main` via dotenvy before this runs).
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    /// Comma-separated origin allowlist for CORS-with-credentials.
    pub allowed_origins: String,
    /// Global JSON body cap. Upload routes get their own, larger, per-route
    /// limit — never widen this one for them.
    pub max_body_size: usize,
    pub production: bool,
    /// Dual gate with `!production`: when both hold, register `/auth/dev-admin`.
    pub allow_dev_admin: bool,
    pub rate_limit: RateLimitConfig,
    /// `local` (default) | `s3` | `minio` | `r2`
    pub storage_backend: String,
    pub storage_local_root: String,
    /// Base URL embedded in local presign stubs.
    pub storage_public_base: String,
    /// Dedicated Gitea code-hosting integration. `None` disables it entirely
    /// (webhook endpoint answers 501, papers require an external repo_url).
    pub gitea: Option<GiteaConfig>,
    /// Base URL of CI-published CLI artifacts.
    pub cli_dist_base: String,
    /// Canonical public origin (sitemap/canonical/OG URLs).
    pub public_base_url: String,
    pub paper_enrichment: EnrichmentConfig,
    /// W-DaaS workspace integration (Research Hub P2). Empty config selects
    /// `StubDaaS` — bind/search/import never block on external infra.
    pub daas: DaasConfig,
}

/// Optional upstreams for best-effort paper metadata. Empty configuration
/// deliberately selects the deterministic stub provider.
#[derive(Debug, Clone, Default)]
pub struct EnrichmentConfig {
    pub openalex_mode: Option<String>,
    pub daas_openalex_base: Option<String>,
    pub w_papers_base: Option<String>,
}

impl EnrichmentConfig {
    pub fn from_env() -> Self {
        Self {
            openalex_mode: std::env::var("OPENALEX_MODE")
                .ok()
                .filter(|v| v == "public"),
            daas_openalex_base: nonempty_env("DAAS_OPENALEX_BASE"),
            w_papers_base: nonempty_env("WPAPERS_BASE"),
        }
    }
}

/// W-DaaS attach point for project bindings, document import, and hybrid
/// search. Absent `DAAS_API_BASE` deliberately selects the deterministic stub.
#[derive(Debug, Clone, Default)]
pub struct DaasConfig {
    /// W-DaaS API base (e.g. `https://data.myfastools.com`).
    pub api_base: Option<String>,
    /// Optional bearer token forwarded to the DaaS API.
    pub api_token: Option<String>,
}

impl DaasConfig {
    pub fn from_env() -> Self {
        Self {
            api_base: nonempty_env("DAAS_API_BASE"),
            api_token: nonempty_env("DAAS_API_TOKEN"),
        }
    }
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Env contract for the dedicated Gitea instance (see `deploy/gitea/README.md`).
/// All four variables must be present together; a partial set is a
/// misconfiguration and fails startup loudly.
#[derive(Debug, Clone)]
pub struct GiteaConfig {
    pub url: String,
    pub token: String,
    pub org: String,
    pub webhook_secret: String,
}

impl GiteaConfig {
    fn from_env() -> anyhow::Result<Option<Self>> {
        let vars = [
            std::env::var("GITEA_URL").ok(),
            std::env::var("GITEA_TOKEN").ok(),
            std::env::var("GITEA_ORG").ok(),
            std::env::var("GITEA_WEBHOOK_SECRET").ok(),
        ];
        let present = vars.iter().flatten().count();
        match present {
            0 => Ok(None),
            4 => {
                let [url, token, org, secret] = vars.map(Option::unwrap);
                Ok(Some(Self {
                    url,
                    token,
                    org,
                    webhook_secret: secret,
                }))
            }
            _ => anyhow::bail!(
                "partial GITEA_* configuration: set all of GITEA_URL, GITEA_TOKEN, \
                 GITEA_ORG, GITEA_WEBHOOK_SECRET or none"
            ),
        }
    }
}

/// Token-bucket parameters for `tower_governor`. Not hard-coded in the layer.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Sustained requests per second for the general (read-friendly) bucket.
    pub rps: u64,
    pub burst: u32,
    /// Stricter bucket applied only to write methods (POST/PUT/PATCH/DELETE).
    pub write_rps: u64,
    pub write_burst: u32,
    /// Strictest bucket: anonymous feedback submission only.
    pub feedback_rps: u64,
    pub feedback_burst: u32,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8030".into())
            .parse()?;
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
        let allowed_origins =
            std::env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "https://auto.ml".into());
        let max_body_size = std::env::var("MAX_BODY_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2 * 1024 * 1024);
        let production = std::env::var("AUTOML_ENV").as_deref() == Ok("production");
        let allow_dev_admin = std::env::var("ALLOW_DEV_ADMIN").as_deref() == Ok("1");
        let rate_limit = RateLimitConfig {
            rps: env_u64("RATE_LIMIT_RPS", 60),
            burst: env_u32("RATE_LIMIT_BURST", 120),
            write_rps: env_u64("RATE_LIMIT_WRITE_RPS", 10),
            write_burst: env_u32("RATE_LIMIT_WRITE_BURST", 20),
            feedback_rps: env_u64("RATE_LIMIT_FEEDBACK_RPS", 1),
            feedback_burst: env_u32("RATE_LIMIT_FEEDBACK_BURST", 5),
        };
        let storage_backend = std::env::var("STORAGE_BACKEND").unwrap_or_else(|_| "local".into());
        let storage_local_root =
            std::env::var("STORAGE_LOCAL_ROOT").unwrap_or_else(|_| "./data/objects".into());
        let storage_public_base =
            std::env::var("STORAGE_PUBLIC_BASE").unwrap_or_else(|_| "http://127.0.0.1:8030".into());
        let gitea = GiteaConfig::from_env()?;
        let cli_dist_base = std::env::var("CLI_DIST_BASE")
            .unwrap_or_else(|_| "https://release.myfastools.com/releases/automl/bin".into());
        let public_base_url = std::env::var("PUBLIC_BASE_URL")
            .unwrap_or_else(|_| "https://auto.ml".into())
            .trim_end_matches('/')
            .to_string();
        let paper_enrichment = EnrichmentConfig::from_env();
        let daas = DaasConfig::from_env();
        Ok(Self {
            bind_addr,
            database_url,
            allowed_origins,
            max_body_size,
            production,
            allow_dev_admin,
            rate_limit,
            storage_backend,
            storage_local_root,
            storage_public_base,
            gitea,
            cli_dist_base,
            public_base_url,
            paper_enrichment,
            daas,
        })
    }

    /// Dev-admin path is registered only when both conditions hold.
    pub fn dev_admin_enabled(&self) -> bool {
        self.allow_dev_admin && !self.production
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_admin_requires_both_flags() {
        let mut cfg = Config {
            bind_addr: "127.0.0.1:1".parse().unwrap(),
            database_url: "postgres://x".into(),
            allowed_origins: "*".into(),
            max_body_size: 1024,
            production: false,
            allow_dev_admin: true,
            rate_limit: RateLimitConfig {
                rps: 1,
                burst: 1,
                write_rps: 1,
                write_burst: 1,
                feedback_rps: 1,
                feedback_burst: 1,
            },
            storage_backend: "local".into(),
            storage_local_root: "/tmp/x".into(),
            storage_public_base: "http://localhost".into(),
            gitea: None,
            cli_dist_base: "https://dist.example.test".into(),
            public_base_url: "https://accept.example.test".into(),
            paper_enrichment: EnrichmentConfig::default(),
            daas: DaasConfig::default(),
        };
        assert!(cfg.dev_admin_enabled());
        cfg.production = true;
        assert!(!cfg.dev_admin_enabled());
        cfg.production = false;
        cfg.allow_dev_admin = false;
        assert!(!cfg.dev_admin_enabled());
    }
}
