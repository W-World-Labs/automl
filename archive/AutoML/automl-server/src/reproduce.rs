// ============================================================
// Reproduction engine — CodeOcean-style reproducible compute.
//
// A paper's reproduction job is a *capsule*: a working directory plus a
// runtime command (from reproduce.yaml). The job is queued in the
// `reproduction_jobs` table; a background worker claims it, runs the
// capsule in a sandbox, captures exit code / stdout / metrics / output
// hashes as evidence, and writes a repro_score back onto the paper.
//
// Two backends, selected by `Config::reproduce_backend`:
//   - "local"       LocalRunner: a plain subprocess on this host. Good
//                   enough to actually run Python in dev; NOT hardened
//                   (no real network/FS isolation).
//   - "cubesandbox" DriverRunner: shells out to an external driver
//                   (default `python3 scripts/cube_reproduce.py`) that
//                   drives Tencent CubeSandbox (KVM-isolated, E2B-compat)
//                   on the configured sandbox host and returns a JSON outcome.
//
// MVP scope (kindergarten): CPU-only, single capsule, the `command`
// carried on the job. Materializing a full source package from object
// storage is a follow-up (needs a known capsule key / tarball).
// ============================================================

use std::time::Duration;

use serde_json::{json, Value};
use tokio::process::Command;

use crate::routes::AppState;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Hard cap on captured stdout/stderr kept as a log excerpt (chars).
const LOG_TAIL_LEN: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Passed,
    Failed,
    Error,
    /// Not executed (e.g. unsandboxed execution disabled in production).
    Skipped,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Passed => "passed",
            JobStatus::Failed => "failed",
            JobStatus::Error => "error",
            JobStatus::Skipped => "skipped",
        }
    }

    /// Map a terminal outcome to a 0-100 reproducibility score.
    pub fn repro_score(self) -> i64 {
        match self {
            JobStatus::Passed => 100,
            JobStatus::Failed => 30,
            JobStatus::Error => 0,
            JobStatus::Skipped => 0,
        }
    }
}

/// A self-contained unit of work handed to a runner.
pub struct Capsule {
    pub paper_id: String,
    pub runtime: String,
    pub command: String,
    pub timeout_secs: u64,
    pub allow_network: bool,
}

/// Structured result of running a capsule.
pub struct RunOutcome {
    pub status: JobStatus,
    pub exit_code: Option<i64>,
    pub log_excerpt: String,
    pub metrics: Value,
    pub output_hashes: Value,
    pub sandbox_id: Option<String>,
}

impl RunOutcome {
    fn error(message: impl Into<String>) -> Self {
        RunOutcome {
            status: JobStatus::Error,
            exit_code: None,
            log_excerpt: message.into(),
            metrics: json!({}),
            output_hashes: json!({}),
            sandbox_id: None,
        }
    }

    fn skipped(message: impl Into<String>) -> Self {
        RunOutcome {
            status: JobStatus::Skipped,
            exit_code: None,
            log_excerpt: message.into(),
            metrics: json!({}),
            output_hashes: json!({}),
            sandbox_id: None,
        }
    }
}

/// Keep only the last `LOG_TAIL_LEN` chars of a combined log.
fn tail(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= LOG_TAIL_LEN {
        s.to_string()
    } else {
        chars[chars.len() - LOG_TAIL_LEN..].iter().collect()
    }
}

// ── Public entry: run one capsule with the configured backend ────────

pub async fn run_capsule(state: &AppState, capsule: &Capsule) -> RunOutcome {
    // Safety gate: never run untrusted, agent-supplied commands on the host
    // without isolation. In production with the local backend this is off, so
    // jobs are recorded as skipped until CubeSandbox provides real isolation.
    if !state.config.reproduce_execute {
        return RunOutcome::skipped(
            "reproduction execution disabled: unsandboxed local backend is not \
             permitted in production — configure the CubeSandbox backend",
        );
    }

    match state.config.reproduce_backend.as_str() {
        "cubesandbox" => run_via_driver(state, capsule).await,
        _ => run_local(capsule).await,
    }
}

// ── Local subprocess runner (dev backend) ────────────────────────────

async fn run_local(capsule: &Capsule) -> RunOutcome {
    let workspace = match make_workspace(&capsule.paper_id) {
        Ok(path) => path,
        Err(e) => return RunOutcome::error(format!("workspace setup failed: {e}")),
    };

    let result = run_shell(&workspace, &capsule.command, capsule.timeout_secs).await;
    let outcome = match result {
        Ok((exit_code, log)) => finalize(&workspace, exit_code, log, None),
        Err(e) => RunOutcome::error(format!("local run failed: {e}")),
    };

    // Best-effort cleanup; ignore errors.
    let _ = std::fs::remove_dir_all(&workspace);
    outcome
}

/// Run `command` via `sh -c` in `workspace` with a wall-clock timeout.
async fn run_shell(
    workspace: &std::path::Path,
    command: &str,
    timeout_secs: u64,
) -> anyhow::Result<(Option<i64>, String)> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(workspace)
        .kill_on_drop(true);

    let child = cmd.output();
    let output = match tokio::time::timeout(Duration::from_secs(timeout_secs), child).await {
        Ok(res) => res?,
        Err(_) => {
            return Ok((
                None,
                format!("timed out after {timeout_secs}s (wall clock)"),
            ));
        }
    };

    let mut log = String::new();
    log.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        log.push_str("\n--- stderr ---\n");
        log.push_str(&stderr);
    }

    Ok((output.status.code().map(|c| c as i64), tail(&log)))
}

/// Turn an exit code + log into an outcome, collecting evidence from the
/// workspace (metrics.json + output file hashes).
fn finalize(
    workspace: &std::path::Path,
    exit_code: Option<i64>,
    log: String,
    sandbox_id: Option<String>,
) -> RunOutcome {
    let metrics = read_metrics(workspace);
    let output_hashes = hash_outputs(workspace);
    let status = match exit_code {
        Some(0) => JobStatus::Passed,
        Some(_) => JobStatus::Failed,
        None => JobStatus::Error,
    };
    RunOutcome {
        status,
        exit_code,
        log_excerpt: log,
        metrics,
        output_hashes,
        sandbox_id,
    }
}

fn read_metrics(workspace: &std::path::Path) -> Value {
    match std::fs::read_to_string(workspace.join("metrics.json")) {
        Ok(text) => serde_json::from_str(&text).unwrap_or(json!({})),
        Err(_) => json!({}),
    }
}

/// Hash conventional output artifacts: metrics.json and anything written
/// under `outputs/`. Keyed by relative path → lowercase-hex SHA-256.
fn hash_outputs(workspace: &std::path::Path) -> Value {
    let mut map = serde_json::Map::new();

    if let Ok(bytes) = std::fs::read(workspace.join("metrics.json")) {
        map.insert(
            "metrics.json".to_string(),
            json!(w_crypto::hash::sha256_hex(&bytes)),
        );
    }

    let outputs_dir = workspace.join("outputs");
    if let Ok(entries) = std::fs::read_dir(&outputs_dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                if let Ok(bytes) = std::fs::read(entry.path()) {
                    let name = format!("outputs/{}", entry.file_name().to_string_lossy());
                    map.insert(name, json!(w_crypto::hash::sha256_hex(&bytes)));
                }
            }
        }
    }

    Value::Object(map)
}

fn make_workspace(paper_id: &str) -> anyhow::Result<std::path::PathBuf> {
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let dir = std::env::temp_dir().join(format!("automl-repro-{paper_id}-{unique}"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ── CubeSandbox driver runner (production backend on the configured sandbox host) ───────
//
// We do not hand-roll the CubeSandbox/E2B wire protocol in Rust. Instead
// we invoke a small external driver and exchange JSON, so the
// vendor-specific bits live in a swappable script next to the sandbox
// service. The driver receives the workspace path as argv[1] and a
// spec.json in the workspace; it prints a single JSON object to stdout.

async fn run_via_driver(state: &AppState, capsule: &Capsule) -> RunOutcome {
    let workspace = match make_workspace(&capsule.paper_id) {
        Ok(path) => path,
        Err(e) => return RunOutcome::error(format!("workspace setup failed: {e}")),
    };

    let spec = json!({
        "paper_id": capsule.paper_id,
        "runtime": capsule.runtime,
        "command": capsule.command,
        "timeout_secs": capsule.timeout_secs,
        "allow_network": capsule.allow_network,
        "allow_local_fallback": state.config.cube_allow_local_fallback,
        "cube_sandbox_url": state.config.cube_sandbox_url,
        "cube_sandbox_api_key": state.config.cube_sandbox_api_key,
    });
    if let Err(e) = std::fs::write(
        workspace.join("spec.json"),
        serde_json::to_vec_pretty(&spec).unwrap_or_default(),
    ) {
        let _ = std::fs::remove_dir_all(&workspace);
        return RunOutcome::error(format!("failed to write spec.json: {e}"));
    }

    let driver = &state.config.cube_reproduce_cmd;
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(format!("{driver} \"$1\""))
        .arg("sh") // $0
        .arg(workspace.to_string_lossy().to_string()) // $1
        .kill_on_drop(true);

    // Driver gets the capsule timeout plus a small margin for sandbox spin-up.
    let driver_timeout = capsule.timeout_secs + 60;
    let outcome =
        match tokio::time::timeout(Duration::from_secs(driver_timeout), cmd.output()).await {
            Ok(Ok(output)) => parse_driver_output(&output.stdout, &output.stderr),
            Ok(Err(e)) => RunOutcome::error(format!("cube driver spawn failed: {e}")),
            Err(_) => RunOutcome::error(format!("cube driver timed out after {driver_timeout}s")),
        };

    let _ = std::fs::remove_dir_all(&workspace);
    outcome
}

fn parse_driver_output(stdout: &[u8], stderr: &[u8]) -> RunOutcome {
    let text = String::from_utf8_lossy(stdout);
    // The driver prints one JSON object on the last non-empty line.
    let line = text.lines().rev().find(|l| !l.trim().is_empty());
    let parsed: Option<Value> = line.and_then(|l| serde_json::from_str(l).ok());

    let Some(v) = parsed else {
        let err = String::from_utf8_lossy(stderr);
        return RunOutcome::error(format!(
            "cube driver returned no JSON outcome. stderr: {}",
            tail(&err)
        ));
    };

    let status = match v.get("status").and_then(Value::as_str) {
        Some("passed") => JobStatus::Passed,
        Some("failed") => JobStatus::Failed,
        _ => JobStatus::Error,
    };

    RunOutcome {
        status,
        exit_code: v.get("exit_code").and_then(Value::as_i64),
        log_excerpt: tail(
            v.get("stdout_tail")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        metrics: v.get("metrics").cloned().unwrap_or(json!({})),
        output_hashes: v.get("output_hashes").cloned().unwrap_or(json!({})),
        sandbox_id: v
            .get("sandbox_id")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

// ── Background worker ────────────────────────────────────────────────

/// Spawn the reproduction worker loop. Idempotent to start once at boot.
pub fn spawn_worker(state: AppState) {
    tokio::spawn(async move {
        tracing::info!(
            backend = %state.config.reproduce_backend,
            "reproduction worker started"
        );
        loop {
            match claim_and_run_one(&state).await {
                Ok(true) => { /* did work; immediately look for more */ }
                Ok(false) => tokio::time::sleep(POLL_INTERVAL).await,
                Err(e) => {
                    tracing::warn!("reproduction worker error: {e}");
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            }
        }
    });
}

/// Atomically claim the oldest queued job (SKIP LOCKED so multiple
/// workers/instances don't double-run), execute it, and persist results.
/// Returns Ok(true) if a job was processed, Ok(false) if the queue was empty.
async fn claim_and_run_one(state: &AppState) -> anyhow::Result<bool> {
    let claimed: Option<(String, String, String, String)> = sqlx::query_as(
        "UPDATE reproduction_jobs \
         SET status = 'running', started_at = CURRENT_TIMESTAMP::text \
         WHERE id = ( \
             SELECT id FROM reproduction_jobs \
             WHERE status = 'queued' \
             ORDER BY created_at ASC \
             FOR UPDATE SKIP LOCKED \
             LIMIT 1 \
         ) \
         RETURNING id, paper_id, runtime, command",
    )
    .fetch_optional(&state.db)
    .await?;

    let Some((job_id, paper_id, runtime, command)) = claimed else {
        return Ok(false);
    };

    tracing::info!(job_id, paper_id, "running reproduction job");

    let capsule = Capsule {
        paper_id: paper_id.clone(),
        runtime,
        command,
        timeout_secs: state.config.reproduce_timeout_secs,
        allow_network: false,
    };

    let outcome = run_capsule(state, &capsule).await;
    persist_outcome(state, &job_id, &paper_id, &outcome).await?;
    Ok(true)
}

async fn persist_outcome(
    state: &AppState,
    job_id: &str,
    paper_id: &str,
    outcome: &RunOutcome,
) -> anyhow::Result<()> {
    let score = outcome.status.repro_score();

    sqlx::query(
        "UPDATE reproduction_jobs SET \
            status = $1, exit_code = $2, repro_score = $3, \
            metrics_json = $4, output_hashes = $5, log_excerpt = $6, \
            sandbox_id = $7, finished_at = CURRENT_TIMESTAMP::text \
         WHERE id = $8",
    )
    .bind(outcome.status.as_str())
    .bind(outcome.exit_code)
    .bind(score)
    .bind(outcome.metrics.to_string())
    .bind(outcome.output_hashes.to_string())
    .bind(&outcome.log_excerpt)
    .bind(&outcome.sandbox_id)
    .bind(job_id)
    .execute(&state.db)
    .await?;

    // Reflect the score on the paper so the rest of the platform (status,
    // silicon-scholar, CLI `reproduce status`) sees the result. A skipped job
    // never ran, so it must not clobber an existing score.
    if outcome.status != JobStatus::Skipped {
        sqlx::query(
            "UPDATE papers SET repro_score = $1, updated_at = CURRENT_TIMESTAMP::text WHERE id = $2",
        )
        .bind(score)
        .bind(paper_id)
        .execute(&state.db)
        .await?;
    }

    let event_id = format!("act_{}", uuid::Uuid::new_v4().simple());
    let message = format!(
        "Reproduction {} (score {}/100)",
        outcome.status.as_str(),
        score
    );
    sqlx::query(
        "INSERT INTO activity_events (id, type, message, actor_id, paper_id, created_at) \
         VALUES ($1, 'reproduction', $2, 'reproduction-worker', $3, CURRENT_TIMESTAMP::text)",
    )
    .bind(&event_id)
    .bind(&message)
    .bind(paper_id)
    .execute(&state.db)
    .await?;

    tracing::info!(
        job_id,
        paper_id,
        status = outcome.status.as_str(),
        score,
        "reproduction job finished"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_runner_executes_python_and_passes() {
        let capsule = Capsule {
            paper_id: "test_paper".to_string(),
            runtime: "python3".to_string(),
            command: "python3 -c \"import json; open('metrics.json','w').write(json.dumps({'acc': 0.9}))\""
                .to_string(),
            timeout_secs: 30,
            allow_network: false,
        };

        let outcome = run_local(&capsule).await;

        assert_eq!(
            outcome.status,
            JobStatus::Passed,
            "log: {}",
            outcome.log_excerpt
        );
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.metrics["acc"], 0.9);
        assert!(outcome.output_hashes.get("metrics.json").is_some());
        assert_eq!(outcome.status.repro_score(), 100);
    }

    #[tokio::test]
    async fn local_runner_marks_nonzero_exit_as_failed() {
        let capsule = Capsule {
            paper_id: "test_paper".to_string(),
            runtime: "python3".to_string(),
            command: "exit 7".to_string(),
            timeout_secs: 30,
            allow_network: false,
        };

        let outcome = run_local(&capsule).await;

        assert_eq!(outcome.status, JobStatus::Failed);
        assert_eq!(outcome.exit_code, Some(7));
        assert_eq!(outcome.status.repro_score(), 30);
    }

    #[tokio::test]
    async fn local_runner_times_out() {
        let capsule = Capsule {
            paper_id: "test_paper".to_string(),
            runtime: "sh".to_string(),
            command: "sleep 30".to_string(),
            timeout_secs: 1,
            allow_network: false,
        };

        let outcome = run_local(&capsule).await;

        assert_eq!(outcome.status, JobStatus::Error);
        assert!(outcome.log_excerpt.contains("timed out"));
    }
}
