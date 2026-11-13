//! Object storage factory — local FS (dev/test) or S3/MinIO (prod).

use std::{path::PathBuf, sync::Arc};

use w_storage::{LocalConfig, LocalStorage, S3Config, S3Storage, Storage};

use crate::config::Config;

/// Build storage from env-driven config. Defaults to local nested FS so
/// integration tests and single-node dev need no MinIO.
pub async fn build_storage(cfg: &Config) -> anyhow::Result<Arc<dyn Storage>> {
    match cfg.storage_backend.as_str() {
        "s3" | "minio" | "r2" => {
            let s3 = S3Config::from_env_prefix("STORAGE")
                .or_else(|_| S3Config::from_env_prefix("S3"))
                .map_err(|e| anyhow::anyhow!("S3 storage config: {e}"))?;
            let store = S3Storage::new(s3)
                .await
                .map_err(|e| anyhow::anyhow!("S3 storage init: {e}"))?;
            Ok(Arc::new(store))
        }
        _ => {
            let root = PathBuf::from(&cfg.storage_local_root);
            tokio::fs::create_dir_all(&root).await?;
            let store = LocalStorage::new(LocalConfig {
                root,
                public_base_url: Some(cfg.storage_public_base.clone()),
                flat_keys_only: false,
                ..LocalConfig::default()
            })
            .await
            .map_err(|e| anyhow::anyhow!("local storage init: {e}"))?;
            Ok(Arc::new(store))
        }
    }
}
