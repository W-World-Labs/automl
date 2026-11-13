// ============================================================
// Auto.ml CLI — Configuration Management
// ============================================================

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_api")]
    pub api: ApiConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_url")]
    pub url: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    pub api_key: Option<String>,
    pub scholar_id: Option<String>,
    pub trust_level: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DefaultsConfig {
    #[serde(default = "default_journal")]
    pub journal: String,
    #[serde(default = "default_compute_tier")]
    pub compute_tier: String,
}

fn default_url() -> String {
    "https://auto.ml".to_string()
}
fn default_journal() -> String {
    "kindergarten".to_string()
}
fn default_compute_tier() -> String {
    "Standard".to_string()
}
fn default_api() -> ApiConfig {
    ApiConfig { url: default_url() }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api: default_api(),
            auth: AuthConfig::default(),
            defaults: DefaultsConfig::default(),
        }
    }
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            journal: default_journal(),
            compute_tier: default_compute_tier(),
        }
    }
}

impl Config {
    /// Get the config file path: ~/.config/automl/config.toml
    pub fn path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("automl");
        Ok(config_dir.join("config.toml"))
    }

    /// Load config from disk, or create default
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let config: Config = toml::from_str(&content)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Save config to disk (with restricted permissions for API key security)
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;

        // Restrict file permissions to owner-only (0600) — protects API key
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    /// Set a config value by dotted key path
    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "api.url" => self.api.url = value.to_string(),
            "auth.api_key" => self.auth.api_key = Some(value.to_string()),
            "auth.scholar_id" => self.auth.scholar_id = Some(value.to_string()),
            "defaults.journal" => self.defaults.journal = value.to_string(),
            "defaults.compute_tier" => self.defaults.compute_tier = value.to_string(),
            _ => anyhow::bail!("Unknown config key: {key}"),
        }
        Ok(())
    }
}
