//! On-disk credential + device metadata files under `~/.config/automl/`.
//! Everything sensitive is written 0600. Format is a flat `key = "value"`
//! subset of TOML that we both write and parse (no external dep).

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub fn config_dir() -> PathBuf {
    std::env::var("AUTOML_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config/automl")
        })
}

pub fn credentials_path() -> PathBuf {
    config_dir().join("credentials.toml")
}

pub fn device_meta_path() -> PathBuf {
    config_dir().join("device.toml")
}

pub fn default_device_key_path() -> PathBuf {
    config_dir().join("device.key")
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub server: String,
    pub api_key: String,
    pub key_id: String,
}

pub fn load_credentials() -> Option<Credentials> {
    let map = read_kv(&credentials_path())?;
    Some(Credentials {
        server: map.get("server")?.clone(),
        api_key: map.get("api_key")?.clone(),
        key_id: map.get("key_id").cloned().unwrap_or_default(),
    })
}

pub fn save_credentials(c: &Credentials) -> anyhow::Result<()> {
    write_kv_0600(
        &credentials_path(),
        &[
            ("server", c.server.as_str()),
            ("api_key", c.api_key.as_str()),
            ("key_id", c.key_id.as_str()),
        ],
    )
}

pub fn delete_credentials() -> bool {
    fs::remove_file(credentials_path()).is_ok()
}

#[derive(Debug, Clone)]
pub struct DeviceMeta {
    pub agent_id: String,
    pub device_id: String,
    pub key_file: PathBuf,
}

pub fn load_device_meta() -> Option<DeviceMeta> {
    let map = read_kv(&device_meta_path())?;
    Some(DeviceMeta {
        agent_id: map.get("agent_id")?.clone(),
        device_id: map.get("device_id")?.clone(),
        key_file: PathBuf::from(map.get("key_file")?),
    })
}

pub fn save_device_meta(m: &DeviceMeta) -> anyhow::Result<()> {
    write_kv_0600(
        &device_meta_path(),
        &[
            ("agent_id", m.agent_id.as_str()),
            ("device_id", m.device_id.as_str()),
            ("key_file", &m.key_file.to_string_lossy()),
        ],
    )
}

pub fn write_private_0600(path: &Path, contents: &str) -> anyhow::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(contents.as_bytes())?;
    f.write_all(b"\n")?;
    // Truncate-open of a pre-existing file keeps old perms — enforce anyway.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn write_kv_0600(path: &Path, pairs: &[(&str, &str)]) -> anyhow::Result<()> {
    let body: String = pairs
        .iter()
        .map(|(k, v)| format!("{k} = \"{v}\"\n"))
        .collect();
    write_private_0600(path, body.trim_end())
}

fn read_kv(path: &Path) -> Option<BTreeMap<String, String>> {
    let text = fs::read_to_string(path).ok()?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"');
            map.insert(k.trim().to_string(), v.to_string());
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_roundtrip_and_0600() {
        let dir = std::env::temp_dir().join(format!("automl-creds-test-{}", std::process::id()));
        std::env::set_var("AUTOML_CONFIG_DIR", &dir);
        let creds = Credentials {
            server: "https://auto.ml".into(),
            api_key: "amk_deadbeef_secret".into(),
            key_id: "8b0d38cb-0000-0000-0000-000000000000".into(),
        };
        save_credentials(&creds).unwrap();

        let mode = fs::metadata(credentials_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "credentials must be 0600");

        let loaded = load_credentials().unwrap();
        assert_eq!(loaded.server, creds.server);
        assert_eq!(loaded.api_key, creds.api_key);
        assert_eq!(loaded.key_id, creds.key_id);

        assert!(delete_credentials());
        assert!(load_credentials().is_none());
        let _ = fs::remove_dir_all(dir);
        std::env::remove_var("AUTOML_CONFIG_DIR");
    }
}
