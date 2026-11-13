// ============================================================
// R2/S3 Storage client
//
// Thin adapter over the shared `w-storage` crate (S3-compatible
// backend, rustls-on-ring — no aws-lc-sys / cmake). The public
// surface of this module (`Storage` struct + its methods) is kept
// byte-for-byte compatible with the previous direct `aws-sdk-s3`
// implementation so every call site (routes/assets.rs,
// routes/papers.rs, routes/system.rs, main.rs) compiles unchanged.
// ============================================================

use crate::config::Config;
use std::time::Duration;
use w_storage::{CredentialMode, S3Config, S3Storage, Storage as _};

#[derive(Clone)]
pub struct Storage {
    inner: std::sync::Arc<S3Storage>,
    /// Bucket name, exposed for health/diagnostics (system.rs reads this).
    pub bucket: String,
}

impl Storage {
    pub async fn new(cfg: &Config) -> Self {
        // R2: static creds, literal "auto" region, path-style addressing —
        // preserved exactly from the previous direct-SDK config.
        let s3_cfg = S3Config {
            endpoint: Some(cfg.r2_endpoint.clone()),
            region: "auto".to_string(),
            bucket: cfg.r2_bucket.clone(),
            credentials: CredentialMode::Static {
                access_key_id: cfg.r2_access_key.clone(),
                secret_access_key: cfg.r2_secret_key.clone(),
                session_token: None,
            },
            force_path_style: true,
            key_prefix: None,
            public_base_url: None,
            label: "r2".to_string(),
            default_ttl_secs: 600,
        };

        // S3Storage::new only fails on credential/endpoint construction
        // problems; the previous `Client::from_conf` path was infallible, so
        // we keep `new` infallible too and surface any error as a panic at
        // startup (same blast radius as a bad config).
        let inner = S3Storage::new(s3_cfg)
            .await
            .expect("failed to construct R2/S3 storage backend");

        Self {
            inner: std::sync::Arc::new(inner),
            bucket: cfg.r2_bucket.clone(),
        }
    }

    /// Upload bytes to R2
    pub async fn put(&self, key: &str, body: Vec<u8>, content_type: &str) -> anyhow::Result<()> {
        self.inner
            .put(key, bytes::Bytes::from(body), Some(content_type))
            .await
            .map_err(|e| anyhow::anyhow!("S3 error: {e}"))?;
        Ok(())
    }

    /// Get object from R2.
    ///
    /// Returns `Ok(None)` when the object is absent (NoSuchKey), matching the
    /// previous `is_no_such_key()` -> `Ok(None)` semantics; any other backend
    /// failure is surfaced as `Err`.
    pub async fn get(&self, key: &str) -> anyhow::Result<Option<(Vec<u8>, String)>> {
        match self.inner.get(key).await {
            Ok((body, ct)) => Ok(Some((body.to_vec(), ct))),
            Err(e) if e.is_not_found() => Ok(None),
            Err(e) => Err(anyhow::anyhow!("S3 error: {e}")),
        }
    }

    /// Generate a presigned PUT URL for direct client upload to R2.
    pub async fn presign_upload(
        &self,
        key: &str,
        content_type: &str,
        expires_secs: u64,
    ) -> anyhow::Result<String> {
        self.inner
            .presign_put(key, Some(content_type), Duration::from_secs(expires_secs))
            .await
            .map_err(|e| anyhow::anyhow!("presign error: {e}"))
    }

    /// Generate a presigned GET URL for direct client download from R2.
    pub async fn presign_download(&self, key: &str, expires_secs: u64) -> anyhow::Result<String> {
        self.inner
            .presign_get(key, None, Duration::from_secs(expires_secs))
            .await
            .map_err(|e| anyhow::anyhow!("presign error: {e}"))
    }

    /// Backend reachability probe (bucket head). Replaces the previous raw
    /// `client.head_bucket()` chain that leaked the SDK type into system.rs.
    pub async fn health(&self) -> anyhow::Result<()> {
        self.inner
            .health()
            .await
            .map_err(|e| anyhow::anyhow!("S3 error: {e}"))
    }
}
