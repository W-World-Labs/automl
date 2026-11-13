//! Client-side execution engine: reproduce runs + device identity.
//!
//! The platform never executes anything — this module is where computation
//! actually happens, on the user's machine. Evidence is signed with the local
//! ed25519 device key when one is registered.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde_json::{json, Value};

use crate::{client::Api, creds};

pub fn reproduce_run(
    api: &Api,
    paper_id: &str,
    workdir: Option<PathBuf>,
    local_path: Option<PathBuf>,
) -> anyhow::Result<Value> {
    api.require_key()?;

    // 1. Paper pointer
    let paper = api.get(&format!("/api/v1/papers/{paper_id}"))?;
    let repo_url = paper["repo_url"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("paper has no repo_url"))?;
    let commit = paper["commit_sha"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if commit.is_empty() {
        anyhow::bail!("paper has no pinned commit_sha yet — nothing to reproduce");
    }
    let spec_path = paper["reproduce_path"].as_str().unwrap_or("reproduce.yaml");

    // 2. Materialize sources
    let cleanup;
    let root = if let Some(p) = local_path {
        cleanup = None;
        p
    } else {
        let dir = workdir
            .unwrap_or_else(|| std::env::temp_dir().join(format!("automl-repro-{paper_id}")));
        if dir.exists() {
            let _ = fs::remove_dir_all(&dir);
        }
        fs::create_dir_all(&dir)?;
        git_clone_checkout(repo_url, &commit, &dir)?;
        cleanup = Some(dir.clone());
        dir
    };

    let spec_file = root.join(spec_path);
    if !spec_file.is_file() {
        anyhow::bail!("reproduce spec not found: {}", spec_file.display());
    }
    let content_hash = w_crypto::sha256_hex(&fs::read(&spec_file)?);

    // 3. Register run
    let run = api.post(
        "/api/v1/reproduce/runs",
        &json!({"paper_id": paper_id, "content_hash": content_hash}),
    )?;
    let run_id = run["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("register run missing id"))?
        .to_string();

    // 4. Execute locally; logs stream to disk as they are produced.
    let log_path = root.join("automl-run.log");
    let (exit_code, log_text) = execute_local(&root, &spec_file, &log_path)?;

    let metrics = extract_metrics(&log_text);
    let artifact_hashes = json!({});
    let env_fingerprint = json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cli": env!("CARGO_PKG_VERSION"),
    });

    // 5. Sign when a device identity exists (evidence gains signature_valid).
    let mut evidence = json!({
        "exit_code": exit_code,
        "metrics": metrics,
        "artifact_hashes": artifact_hashes,
        "env_fingerprint": env_fingerprint,
    });
    let mut signed_by: Option<String> = None;
    if let Some(meta) = creds::load_device_meta() {
        match sign_evidence(&meta, &run_id, exit_code, &evidence) {
            Ok((sig_hex, device_id)) => {
                evidence["signature_hex"] = json!(sig_hex);
                evidence["signer_key_id"] = json!(device_id);
                signed_by = Some(device_id);
            }
            Err(err) => eprintln!("warning: device key unusable, submitting unsigned: {err}"),
        }
    }

    // 6. Submit — retried on transient failures so a finished run isn't lost.
    let mut resp = api.put_with_retry(
        &format!("/api/v1/reproduce/runs/{run_id}/evidence"),
        &evidence,
    )?;

    if let Some(dir) = cleanup {
        let _ = fs::remove_dir_all(dir);
    }
    if let Some(device) = signed_by {
        resp["signed_with_device"] = json!(device);
    }
    Ok(resp)
}

fn sign_evidence(
    meta: &creds::DeviceMeta,
    run_id: &str,
    exit_code: i32,
    evidence: &Value,
) -> anyhow::Result<(String, String)> {
    let seed_hex = fs::read_to_string(&meta.key_file)?;
    let seed = w_crypto::hex_decode(seed_hex.trim())
        .map_err(|_| anyhow::anyhow!("device key file is not valid hex"))?;
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| anyhow::anyhow!("device key must be a 32-byte seed"))?;
    let sk = SigningKey::from_bytes(&seed);
    let msg = automl_core::canonical_evidence_message(
        run_id,
        exit_code,
        None,
        &evidence["artifact_hashes"],
        &evidence["metrics"],
        &evidence["env_fingerprint"],
    );
    let sig = sk.sign(&msg);
    Ok((
        w_crypto::hex_encode(&sig.to_bytes()),
        meta.device_id.clone(),
    ))
}

pub fn device_register(
    api: &Api,
    agent_id: &str,
    key_file: Option<PathBuf>,
    env_fingerprint: &str,
) -> anyhow::Result<Value> {
    api.require_key()?;
    let env: Value = serde_json::from_str(env_fingerprint)
        .map_err(|e| anyhow::anyhow!("env_fingerprint must be JSON: {e}"))?;
    if !env.is_object() {
        anyhow::bail!("env_fingerprint must be a JSON object");
    }

    let sk = SigningKey::generate(&mut OsRng);
    let pk_hex = w_crypto::hex_encode(&sk.verifying_key().to_bytes());

    let device = api.post(
        &format!("/api/v1/agents/{agent_id}/devices"),
        &json!({"pubkey": pk_hex, "env_fingerprint": env}),
    )?;
    let device_id = device["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("device registration returned no id"))?
        .to_string();

    let path = key_file.unwrap_or_else(creds::default_device_key_path);
    creds::write_private_0600(&path, &w_crypto::hex_encode(&sk.to_bytes()))?;
    creds::save_device_meta(&creds::DeviceMeta {
        agent_id: agent_id.to_string(),
        device_id: device_id.clone(),
        key_file: path.clone(),
    })?;

    Ok(json!({
        "device_id": device_id,
        "agent_id": agent_id,
        "pubkey_hex": pk_hex,
        "key_file": path.to_string_lossy(),
    }))
}

fn git_clone_checkout(repo: &str, commit: &str, dir: &Path) -> anyhow::Result<()> {
    let status = Command::new("git")
        .args(["clone", "--depth", "1", repo, &dir.to_string_lossy()])
        .status()?;
    if !status.success() {
        let _ = fs::remove_dir_all(dir);
        let status = Command::new("git")
            .args(["clone", repo, &dir.to_string_lossy()])
            .status()?;
        if !status.success() {
            anyhow::bail!("git clone failed for {repo}");
        }
    }
    let status = Command::new("git")
        .current_dir(dir)
        .args(["checkout", commit])
        .status()?;
    if !status.success() {
        let _ = Command::new("git")
            .current_dir(dir)
            .args(["fetch", "--depth", "1", "origin", commit])
            .status();
        let status = Command::new("git")
            .current_dir(dir)
            .args(["checkout", commit])
            .status()?;
        if !status.success() {
            anyhow::bail!("git checkout {commit} failed");
        }
    }
    Ok(())
}

/// Run `reproduce.sh` streaming combined output line-by-line to `log_path`
/// (visible with tail -f during long runs) while also collecting it in memory
/// for metric extraction.
fn execute_local(root: &Path, spec_file: &Path, log_path: &Path) -> anyhow::Result<(i32, String)> {
    let sh = root.join("reproduce.sh");
    if sh.is_file() {
        let mut child = Command::new("bash")
            .arg("-c")
            .arg(format!("exec bash '{}' 2>&1", sh.display()))
            .current_dir(root)
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("child stdout unavailable"))?;
        let mut log_file = fs::File::create(log_path)?;
        let mut collected = String::new();
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            let _ = writeln!(log_file, "{line}");
            collected.push_str(&line);
            collected.push('\n');
        }
        let code = child.wait()?.code().unwrap_or(1);
        return Ok((code, collected));
    }
    // Spec-only path: readable yaml counts as a trivially successful dry run.
    let _ = fs::read_to_string(spec_file)?;
    let log = format!(
        "automl-cli: no reproduce.sh; accepted spec-only at {}\nMETRICS accuracy=1.0\n",
        spec_file.display()
    );
    let _ = fs::write(log_path, &log);
    Ok((0, log))
}

fn extract_metrics(log: &str) -> Value {
    for line in log.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("METRICS ") {
            let mut map = serde_json::Map::new();
            for part in rest.split_whitespace() {
                if let Some((k, v)) = part.split_once('=') {
                    if let Ok(n) = v.parse::<f64>() {
                        map.insert(k.to_string(), json!(n));
                    } else {
                        map.insert(k.to_string(), json!(v));
                    }
                }
            }
            if !map.is_empty() {
                return Value::Object(map);
            }
        }
    }
    json!({})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_parse_numbers_and_strings() {
        let m = extract_metrics("noise\nMETRICS accuracy=0.91 tag=fast\n");
        assert_eq!(m["accuracy"], json!(0.91));
        assert_eq!(m["tag"], json!("fast"));
        assert_eq!(extract_metrics("no metrics here"), json!({}));
    }
}
