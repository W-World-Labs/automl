// ============================================================
// Configuration — loaded from environment variables
// Supports .env files via dotenvy (loaded in main.rs)
// ============================================================

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub r2_endpoint: String,
    pub r2_bucket: String,
    pub r2_access_key: String,
    pub r2_secret_key: String,
    /// Public base URL for R2 objects (custom domain or r2.dev)
    /// e.g. https://r2.auto.ml or https://pub-xxx.r2.dev
    pub r2_public_url: String,
    pub auth_secret: String,
    pub base_url: String,
    /// Comma-separated allowed origins; "*" = allow all (dev only)
    pub allowed_origins: String,
    /// Comma-separated admin API key hashes (or "aml_xxx" raw keys)
    pub admin_keys: Vec<String>,
    /// Max request body size in bytes (default: 50MB)
    pub max_body_size: usize,
    /// Environment name: dev / staging / prod
    pub env_name: String,
    /// Reproduction backend: "local" (subprocess) or "cubesandbox".
    pub reproduce_backend: String,
    /// Wall-clock timeout for a single reproduction capsule, in seconds.
    pub reproduce_timeout_secs: u64,
    /// CubeSandbox service URL (E2B-compatible) on the configured sandbox host.
    pub cube_sandbox_url: String,
    /// CubeSandbox API key.
    pub cube_sandbox_api_key: String,
    /// External driver command for the cubesandbox backend; receives the
    /// capsule workspace path as its single argument.
    pub cube_reproduce_cmd: String,
    /// Whether the worker may actually execute capsules. The local backend
    /// runs agent-supplied commands directly on the host with no isolation,
    /// so execution is refused in production unless explicitly opted in —
    /// real reproduction in prod requires the CubeSandbox backend.
    pub reproduce_execute: bool,
    /// Whether the cubesandbox driver may fall back to host-local execution
    /// when CubeSandbox is unreachable. Off in production.
    pub cube_allow_local_fallback: bool,
    /// Dual-gated with `!is_production()`: when both hold, empty ADMIN_KEYS
    /// elevates any authenticated user to admin (local-dev convenience only).
    /// Fail-closed unless `ALLOW_DEV_ADMIN=1`.
    pub allow_dev_admin: bool,
}

/// Whether the reproduction worker may execute capsules for `backend`.
/// The local backend has no isolation, so it is refused in production unless
/// explicitly opted in; the cubesandbox backend is always allowed.
pub fn reproduce_should_execute(backend: &str, is_production: bool, allow_local: bool) -> bool {
    match backend {
        "cubesandbox" => true,
        _ => !is_production || allow_local,
    }
}

impl Config {
    pub fn from_env() -> Self {
        // Load .env if present
        let _ = dotenvy::dotenv();

        let env_name = std::env::var("AUTOML_ENV").unwrap_or_else(|_| "dev".into());

        let is_production = env_name == "prod" || env_name == "production";

        // Reproduction backend: default "local" unless a CubeSandbox URL is
        // configured, in which case prefer the hardened backend.
        let reproduce_backend = std::env::var("REPRODUCE_BACKEND").unwrap_or_else(|_| {
            if std::env::var("CUBE_SANDBOX_URL")
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
            {
                "cubesandbox".into()
            } else {
                "local".into()
            }
        });
        let allow_local_exec = env_flag("REPRODUCE_ALLOW_LOCAL_EXEC", false);
        let reproduce_execute =
            reproduce_should_execute(&reproduce_backend, is_production, allow_local_exec);
        // In dev the driver may fall back to host execution for convenience;
        // in prod it must not (that would run untrusted code on the host).
        let cube_allow_local_fallback = env_flag("CUBE_ALLOW_LOCAL_FALLBACK", !is_production);

        let admin_keys_str = std::env::var("ADMIN_KEYS").unwrap_or_default();
        let admin_keys: Vec<String> = if admin_keys_str.is_empty() {
            Vec::new()
        } else {
            admin_keys_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        };
        let allow_dev_admin = std::env::var("ALLOW_DEV_ADMIN").as_deref() == Ok("1");

        Self {
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3100".into()),
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://automl:automl_dev_password@127.0.0.1:55439/automl".into()
            }),
            r2_endpoint: std::env::var("R2_ENDPOINT")
                .unwrap_or_else(|_| "https://placeholder.r2.cloudflarestorage.com".into()),
            r2_bucket: std::env::var("R2_BUCKET").unwrap_or_else(|_| "automl-papers".into()),
            r2_access_key: std::env::var("R2_ACCESS_KEY").unwrap_or_default(),
            r2_secret_key: std::env::var("R2_SECRET_KEY").unwrap_or_default(),
            r2_public_url: std::env::var("R2_PUBLIC_URL").unwrap_or_default(),
            auth_secret: std::env::var("AUTH_SECRET")
                .unwrap_or_else(|_| "automl-dev-secret".into()),
            base_url: std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3100".into()),
            allowed_origins: std::env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "*".into()),
            admin_keys,
            max_body_size: std::env::var("MAX_BODY_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50 * 1024 * 1024), // 50MB default
            env_name,
            reproduce_backend,
            reproduce_timeout_secs: std::env::var("REPRODUCE_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            cube_sandbox_url: std::env::var("CUBE_SANDBOX_URL").unwrap_or_default(),
            cube_sandbox_api_key: std::env::var("CUBE_SANDBOX_API_KEY").unwrap_or_default(),
            cube_reproduce_cmd: std::env::var("CUBE_REPRODUCE_CMD")
                .unwrap_or_else(|_| "python3 scripts/cube_reproduce.py".into()),
            reproduce_execute,
            cube_allow_local_fallback,
            allow_dev_admin,
        }
    }

    pub fn is_production(&self) -> bool {
        self.env_name == "prod" || self.env_name == "production"
    }

    /// Open elevation of authenticated users to admin is dual-gated and
    /// fail-closed: requires `ALLOW_DEV_ADMIN=1` and non-production.
    pub fn dev_admin_elevation_enabled(&self) -> bool {
        self.allow_dev_admin && !self.is_production()
    }

    pub fn secure_cookies(&self) -> bool {
        self.is_production() || self.base_url.starts_with("https://")
    }

    pub fn cookie_domain(&self) -> Option<&'static str> {
        if self.base_url.contains("auto.ml") {
            Some(".auto.ml")
        } else {
            None
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.is_production() {
            return Ok(());
        }

        let mut errors = Vec::new();

        if !self.bind_addr.starts_with("127.0.0.1:") {
            errors.push("BIND_ADDR must bind loopback behind shared Caddy in production");
        }
        if self.database_url.trim().is_empty() {
            errors.push("DATABASE_URL is required");
        }
        if self.allowed_origins.trim() == "*" || self.allowed_origins.trim().is_empty() {
            errors.push("ALLOWED_ORIGINS must list explicit production origins");
        }
        if self.admin_keys.is_empty() {
            errors.push("ADMIN_KEYS must contain at least one admin key or hash");
        }
        if self.auth_secret.trim().is_empty() || self.auth_secret == "automl-dev-secret" {
            errors.push("AUTH_SECRET must be set to a non-default value");
        }
        if is_placeholder(&self.r2_endpoint) {
            errors.push("R2_ENDPOINT must point to the production R2/S3 endpoint");
        }
        if self.r2_bucket.trim().is_empty() || is_placeholder(&self.r2_bucket) {
            errors.push("R2_BUCKET must be set to the production bucket name");
        }
        if self.r2_access_key.trim().is_empty() || is_placeholder(&self.r2_access_key) {
            errors.push("R2_ACCESS_KEY is required");
        }
        if self.r2_secret_key.trim().is_empty() || is_placeholder(&self.r2_secret_key) {
            errors.push("R2_SECRET_KEY is required");
        }
        if is_placeholder(&self.base_url) {
            errors.push("BASE_URL must be the public production URL");
        }
        if !env_present("W_ID_CLIENT_ID") {
            errors.push("W_ID_CLIENT_ID is required for W-ID login");
        }
        if !env_present("W_ID_REDIRECT_URI") {
            errors.push("W_ID_REDIRECT_URI is required for W-ID login");
        }
        if !env_present("W_ID_CLIENT_SECRET") && !env_present("W_ID_CLIENT_SECRET_REF") {
            errors.push("W_ID_CLIENT_SECRET or W_ID_CLIENT_SECRET_REF is required");
        }
        if !env_present("JWT_SECRET") && !env_present("JWT_SECRET_REF") {
            errors.push("JWT_SECRET or JWT_SECRET_REF is required for session cookies");
        }
        if !env_present("W_RADAR_CODE") {
            errors.push("W_RADAR_CODE is required for Radar telemetry");
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "invalid AutoML production configuration:\n- {}",
                errors.join("\n- ")
            )
        }
    }
}

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        }
        Err(_) => default,
    }
}

fn env_present(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn is_placeholder(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || value.contains("placeholder")
        || value.contains("example")
        || value.contains("replace-with")
        || value.contains("localhost")
}

#[cfg(test)]
mod tests {
    use super::{reproduce_should_execute, Config};

    #[test]
    fn local_backend_refused_in_production_by_default() {
        // The dangerous case: prod + unsandboxed local backend must NOT run.
        assert!(!reproduce_should_execute("local", true, false));
    }

    #[test]
    fn local_backend_allowed_in_dev() {
        assert!(reproduce_should_execute("local", false, false));
    }

    #[test]
    fn local_backend_allowed_in_prod_only_with_explicit_optin() {
        assert!(reproduce_should_execute("local", true, true));
    }

    #[test]
    fn cubesandbox_backend_always_allowed() {
        assert!(reproduce_should_execute("cubesandbox", true, false));
    }

    #[test]
    fn dev_admin_elevation_requires_explicit_flag_and_non_production() {
        let mut cfg = Config {
            bind_addr: "127.0.0.1:1".into(),
            database_url: "postgres://x".into(),
            r2_endpoint: String::new(),
            r2_bucket: String::new(),
            r2_access_key: String::new(),
            r2_secret_key: String::new(),
            r2_public_url: String::new(),
            auth_secret: "x".into(),
            base_url: "http://localhost".into(),
            allowed_origins: "*".into(),
            admin_keys: Vec::new(),
            max_body_size: 1024,
            env_name: "dev".into(),
            reproduce_backend: "local".into(),
            reproduce_timeout_secs: 1,
            cube_sandbox_url: String::new(),
            cube_sandbox_api_key: String::new(),
            cube_reproduce_cmd: String::new(),
            reproduce_execute: true,
            cube_allow_local_fallback: true,
            allow_dev_admin: false,
        };
        // Fail closed by default even in non-production with empty ADMIN_KEYS.
        assert!(!cfg.dev_admin_elevation_enabled());
        cfg.allow_dev_admin = true;
        assert!(cfg.dev_admin_elevation_enabled());
        cfg.env_name = "production".into();
        assert!(!cfg.dev_admin_elevation_enabled());
        cfg.env_name = "prod".into();
        assert!(!cfg.dev_admin_elevation_enabled());
    }
}
