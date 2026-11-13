//! Reproduce evidence chain — register run → submit evidence → verified-by-N.
//!
//! Server validates **integrity** only (schema, hashes, submitter). It does not
//! execute code and does not judge scientific truth.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::agents::{self, canonical_evidence_message};
use crate::error::{ApiError, ApiResult};

/// Relative + absolute tolerance for numeric metric consensus.
pub const METRIC_REL_TOL: f64 = 1e-6;
pub const METRIC_ABS_TOL: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Registered,
    EvidenceSubmitted,
    Accepted,
    Rejected,
    /// Timeout terminal state: registered but no evidence arrived before the
    /// sweep deadline. Swept by `mark_stale_runs` (w-task periodic).
    Stale,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::EvidenceSubmitted => "evidence_submitted",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "evidence_submitted" => Self::EvidenceSubmitted,
            "accepted" => Self::Accepted,
            "rejected" => Self::Rejected,
            "stale" => Self::Stale,
            _ => Self::Registered,
        }
    }

    /// Only `registered` moves; everything else is terminal.
    /// (`evidence_submitted` is a reserved intermediate for future async
    /// validation — currently evidence resolves directly to a terminal state.)
    pub fn can_transition(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Registered, Self::EvidenceSubmitted)
                | (Self::Registered, Self::Accepted)
                | (Self::Registered, Self::Rejected)
                | (Self::Registered, Self::Stale)
                | (Self::EvidenceSubmitted, Self::Accepted)
                | (Self::EvidenceSubmitted, Self::Rejected)
        )
    }
}

/// Timeout sweep: registered runs older than `older_than_hours` become
/// `stale` (terminal). Idempotent; driven by a w-task periodic in main.
pub async fn mark_stale_runs(pool: &PgPool, older_than_hours: i64) -> anyhow::Result<u64> {
    let n = sqlx::query(
        r#"
        UPDATE reproduce_runs SET status = 'stale'
        WHERE status = 'registered'
          AND created_at < now() - make_interval(hours => $1)
        "#,
    )
    .bind(older_than_hours as i32)
    .execute(pool)
    .await?
    .rows_affected();
    if n > 0 {
        tracing::info!(count = n, "marked stale reproduce runs");
    }
    Ok(n)
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReproduceRun {
    pub id: Uuid,
    pub paper_id: Uuid,
    pub spec_hash: String,
    pub runner_principal: String,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub evidence_submitted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterRunRequest {
    pub paper_id: Uuid,
    /// SHA-256 hex of reproduce.yaml bytes (client-side). Combined with the
    /// paper's pinned commit_sha into `spec_hash`.
    pub content_hash: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SubmitEvidenceRequest {
    pub exit_code: i32,
    pub logs_object_key: Option<String>,
    #[serde(default)]
    pub artifact_hashes: Value,
    #[serde(default)]
    pub metrics: Value,
    #[serde(default)]
    pub env_fingerprint: Value,
    /// ed25519 signature over the canonical evidence payload (hex).
    /// Required together with `signer_key_id` for verified-by-N credit.
    pub signature_hex: Option<String>,
    /// Registered `agent_devices.id` that holds the verifying public key.
    pub signer_key_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RunEvidence {
    pub run_id: Uuid,
    pub exit_code: i32,
    pub logs_object_key: Option<String>,
    pub artifact_hashes: Value,
    pub metrics: Value,
    pub env_fingerprint: Value,
    pub has_signature: bool,
    pub signature_valid: bool,
    pub env_drift: bool,
    pub signer_key_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReproductionSummary {
    pub paper_id: Uuid,
    pub spec_hash: Option<String>,
    pub runs_total: u64,
    pub distinct_runners: u64,
    pub verified_by_n: u64,
    pub consensus_metrics: Option<Value>,
}

/// `spec_hash = sha256(commit_sha + ":" + content_hash)`.
pub fn compute_spec_hash(commit_sha: &str, content_hash: &str) -> String {
    w_crypto::sha256_hex(format!("{commit_sha}:{content_hash}").as_bytes())
}

pub fn validate_hex64(label: &str, s: &str) -> ApiResult<()> {
    if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::invalid(format!(
            "{label} must be 64 lowercase hex characters"
        )));
    }
    Ok(())
}

pub async fn register_run(
    pool: &PgPool,
    runner_principal: &str,
    req: RegisterRunRequest,
) -> ApiResult<ReproduceRun> {
    validate_hex64("content_hash", &req.content_hash)?;
    let content_hash = req.content_hash.to_ascii_lowercase();

    let paper =
        sqlx::query_as::<_, (Uuid, String)>("SELECT id, commit_sha FROM papers WHERE id = $1")
            .bind(req.paper_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| ApiError::not_found("paper not found"))?;

    let commit_sha = paper.1.trim().to_string();
    if commit_sha.is_empty() {
        // Platform-hosted paper with no pinned commit yet — nothing to reproduce.
        return Err(ApiError::conflict(
            "paper has no pinned commit_sha yet; pin a commit before registering runs",
        ));
    }
    let spec_hash = compute_spec_hash(&commit_sha, &content_hash);

    let row = sqlx::query_as::<_, RunRow>(
        r#"
        INSERT INTO reproduce_runs (paper_id, spec_hash, runner_principal, status)
        VALUES ($1, $2, $3, 'registered')
        RETURNING id, paper_id, spec_hash, runner_principal, status,
                  created_at, evidence_submitted_at
        "#,
    )
    .bind(req.paper_id)
    .bind(&spec_hash)
    .bind(runner_principal)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn list_runs(
    pool: &PgPool,
    params: &automl_core::PageParams,
    paper_id: Option<Uuid>,
) -> ApiResult<automl_core::Page<ReproduceRun>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };
    let rows = sqlx::query_as::<_, RunRow>(
        r#"
        SELECT id, paper_id, spec_hash, runner_principal, status,
               created_at, evidence_submitted_at
        FROM reproduce_runs
        WHERE ($1::uuid IS NULL OR paper_id = $1)
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(paper_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(pool)
    .await?;
    Ok(automl_core::Page::from_rows(
        rows.into_iter().map(ReproduceRun::from).collect(),
        limit,
        |r| super::cursor::encode(r.created_at, r.id),
    ))
}

pub async fn get_run(pool: &PgPool, id: Uuid) -> ApiResult<ReproduceRun> {
    let row = sqlx::query_as::<_, RunRow>(
        r#"
        SELECT id, paper_id, spec_hash, runner_principal, status,
               created_at, evidence_submitted_at
        FROM reproduce_runs WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("reproduce run not found"))?;
    Ok(row.into())
}

pub async fn submit_evidence(
    pool: &PgPool,
    run_id: Uuid,
    runner_principal: &str,
    req: SubmitEvidenceRequest,
) -> ApiResult<(ReproduceRun, RunEvidence)> {
    let run = get_run(pool, run_id).await?;
    if run.runner_principal != runner_principal {
        return Err(ApiError::forbidden(
            "only the registering principal may submit evidence",
        ));
    }
    if run.status != RunStatus::Registered {
        return Err(ApiError::conflict(format!(
            "run is already in status {}",
            run.status.as_str()
        )));
    }

    validate_evidence_schema(&req)?;

    // Signature is optional for storage, but invalid/revoked devices always 422.
    let (signature, signature_valid, env_drift) =
        match (req.signature_hex.as_deref(), req.signer_key_id) {
            (None, None) => (None, false, false),
            (Some(_), None) | (None, Some(_)) => {
                return Err(ApiError::invalid(
                    "signature_hex and signer_key_id must both be set or both omitted",
                ));
            }
            (Some(hex), Some(device_id)) => {
                if !hex.len().is_multiple_of(2) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(ApiError::signature_invalid(
                        "signature_hex must be even-length hex",
                    ));
                }
                let sig = decode_hex(hex)?;
                let device = agents::get_active_device(pool, device_id).await?;
                let msg = canonical_evidence_message(
                    run_id,
                    req.exit_code,
                    &req.logs_object_key,
                    &req.artifact_hashes,
                    &req.metrics,
                    &req.env_fingerprint,
                );
                device.verify(&msg, &sig)?;
                // Drift is informational only — never rejects.
                let drift = req.env_fingerprint != device.env_fingerprint;
                (Some(sig), true, drift)
            }
        };

    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        INSERT INTO run_evidence (
            run_id, exit_code, logs_object_key, artifact_hashes,
            metrics, env_fingerprint, signature, signer_key_id,
            signature_valid, env_drift
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(run_id)
    .bind(req.exit_code)
    .bind(&req.logs_object_key)
    .bind(&req.artifact_hashes)
    .bind(&req.metrics)
    .bind(&req.env_fingerprint)
    .bind(signature.as_deref())
    .bind(req.signer_key_id)
    .bind(signature_valid)
    .bind(env_drift)
    .execute(&mut *tx)
    .await?;

    // Integrity pass: well-formed evidence + exit_code recorded → accepted.
    // Scientific truth is out of scope (verified-by-N aggregates consensus
    // among signature_valid runs only).
    let status = if req.exit_code == 0 {
        RunStatus::Accepted
    } else {
        RunStatus::Rejected
    };

    let row = sqlx::query_as::<_, RunRow>(
        r#"
        UPDATE reproduce_runs
        SET status = $2,
            evidence_submitted_at = now()
        WHERE id = $1
        RETURNING id, paper_id, spec_hash, runner_principal, status,
                  created_at, evidence_submitted_at
        "#,
    )
    .bind(run_id)
    .bind(status.as_str())
    .fetch_one(&mut *tx)
    .await?;

    // Evidence outcome is an outbound event, atomic with the run update.
    super::events::emit(
        &mut tx,
        automl_core::event::REPRO_COMPLETED,
        serde_json::json!({
            "run_id": run_id,
            "paper_id": run.paper_id,
            "status": status.as_str(),
            "spec_hash": run.spec_hash,
            "signature_valid": signature_valid,
        }),
    )
    .await?;

    tx.commit().await?;

    let evidence = RunEvidence {
        run_id,
        exit_code: req.exit_code,
        logs_object_key: req.logs_object_key,
        artifact_hashes: req.artifact_hashes,
        metrics: req.metrics,
        env_fingerprint: req.env_fingerprint,
        has_signature: signature.is_some(),
        signature_valid,
        env_drift,
        signer_key_id: req.signer_key_id,
    };
    Ok((row.into(), evidence))
}

pub async fn reproduction_summary(pool: &PgPool, paper_id: Uuid) -> ApiResult<ReproductionSummary> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM papers WHERE id = $1)")
        .bind(paper_id)
        .fetch_one(pool)
        .await?;
    if !exists {
        return Err(ApiError::not_found("paper not found"));
    }

    let runs_total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reproduce_runs WHERE paper_id = $1")
            .bind(paper_id)
            .fetch_one(pool)
            .await?;

    // Prefer the most frequent accepted spec_hash for the summary focus.
    let focus_spec: Option<String> = sqlx::query_scalar(
        r#"
        SELECT spec_hash FROM reproduce_runs
        WHERE paper_id = $1 AND status = 'accepted'
        GROUP BY spec_hash
        ORDER BY COUNT(*) DESC, spec_hash
        LIMIT 1
        "#,
    )
    .bind(paper_id)
    .fetch_optional(pool)
    .await?;

    let Some(spec_hash) = focus_spec else {
        let distinct: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT runner_principal) FROM reproduce_runs WHERE paper_id = $1",
        )
        .bind(paper_id)
        .fetch_one(pool)
        .await?;
        return Ok(ReproductionSummary {
            paper_id,
            spec_hash: None,
            runs_total: runs_total as u64,
            distinct_runners: distinct as u64,
            verified_by_n: 0,
            consensus_metrics: None,
        });
    };

    // All accepted runs (for distinct_runners).
    let all_rows = sqlx::query_as::<_, (String, Value)>(
        r#"
        SELECT r.runner_principal, e.metrics
        FROM reproduce_runs r
        JOIN run_evidence e ON e.run_id = r.id
        WHERE r.paper_id = $1
          AND r.spec_hash = $2
          AND r.status = 'accepted'
        "#,
    )
    .bind(paper_id)
    .bind(&spec_hash)
    .fetch_all(pool)
    .await?;

    // verified-by-N only credits signature-valid evidence (iteration 4).
    let signed_rows = sqlx::query_as::<_, (String, Value)>(
        r#"
        SELECT r.runner_principal, e.metrics
        FROM reproduce_runs r
        JOIN run_evidence e ON e.run_id = r.id
        WHERE r.paper_id = $1
          AND r.spec_hash = $2
          AND r.status = 'accepted'
          AND e.signature_valid = true
        "#,
    )
    .bind(paper_id)
    .bind(&spec_hash)
    .fetch_all(pool)
    .await?;

    let distinct_runners = all_rows
        .iter()
        .map(|(p, _)| p.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len() as u64;

    let (verified_by_n, consensus_metrics) = compute_verified_by_n(&signed_rows);

    Ok(ReproductionSummary {
        paper_id,
        spec_hash: Some(spec_hash.trim().to_string()),
        runs_total: runs_total as u64,
        distinct_runners,
        verified_by_n,
        consensus_metrics,
    })
}

/// Among accepted runs (already filtered), find the largest set of **distinct**
/// runners whose metrics agree within tolerance. That size is `verified_by_n`.
fn compute_verified_by_n(rows: &[(String, Value)]) -> (u64, Option<Value>) {
    if rows.is_empty() {
        return (0, None);
    }

    // Deduplicate by runner (keep first metrics).
    let mut by_runner: Map<String, Value> = Map::new();
    for (runner, metrics) in rows {
        by_runner
            .entry(runner.clone())
            .or_insert_with(|| metrics.clone());
    }
    let entries: Vec<(String, Value)> = by_runner.into_iter().collect();
    if entries.len() == 1 {
        return (1, Some(entries[0].1.clone()));
    }

    let mut best_n = 1u64;
    let mut best_metrics = entries[0].1.clone();

    for i in 0..entries.len() {
        let mut cluster = 1u64;
        for j in 0..entries.len() {
            if i == j {
                continue;
            }
            if metrics_compatible(&entries[i].1, &entries[j].1) {
                cluster += 1;
            }
        }
        if cluster > best_n {
            best_n = cluster;
            best_metrics = entries[i].1.clone();
        }
    }

    (best_n, Some(best_metrics))
}

/// Structural equality for non-numbers; numeric leaves within rel/abs tol.
pub fn metrics_compatible(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let xf = x.as_f64().unwrap_or(f64::NAN);
            let yf = y.as_f64().unwrap_or(f64::NAN);
            if xf.is_nan() || yf.is_nan() {
                return false;
            }
            let diff = (xf - yf).abs();
            diff <= METRIC_ABS_TOL || diff <= METRIC_REL_TOL * xf.abs().max(yf.abs())
        }
        (Value::Object(xo), Value::Object(yo)) => {
            if xo.len() != yo.len() {
                return false;
            }
            xo.iter()
                .all(|(k, xv)| yo.get(k).is_some_and(|yv| metrics_compatible(xv, yv)))
        }
        (Value::Array(xa), Value::Array(ya)) => {
            xa.len() == ya.len()
                && xa
                    .iter()
                    .zip(ya.iter())
                    .all(|(xv, yv)| metrics_compatible(xv, yv))
        }
        _ => a == b,
    }
}

fn decode_hex(s: &str) -> ApiResult<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(ApiError::invalid("hex length must be even"));
    }
    (0..s.len() / 2)
        .map(|i| {
            u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| ApiError::invalid("invalid hex"))
        })
        .collect()
}

fn validate_evidence_schema(req: &SubmitEvidenceRequest) -> ApiResult<()> {
    if !req.artifact_hashes.is_object() {
        return Err(ApiError::invalid("artifact_hashes must be a JSON object"));
    }
    if !req.metrics.is_object() {
        return Err(ApiError::invalid("metrics must be a JSON object"));
    }
    if !req.env_fingerprint.is_object() {
        return Err(ApiError::invalid("env_fingerprint must be a JSON object"));
    }
    for (name, hash) in req.artifact_hashes.as_object().unwrap() {
        let Some(h) = hash.as_str() else {
            return Err(ApiError::invalid(format!(
                "artifact_hashes.{name} must be a hex string"
            )));
        };
        validate_hex64(&format!("artifact_hashes.{name}"), h)?;
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    paper_id: Uuid,
    spec_hash: String,
    runner_principal: String,
    status: String,
    created_at: DateTime<Utc>,
    evidence_submitted_at: Option<DateTime<Utc>>,
}

impl From<RunRow> for ReproduceRun {
    fn from(r: RunRow) -> Self {
        Self {
            id: r.id,
            paper_id: r.paper_id,
            spec_hash: r.spec_hash.trim().to_string(),
            runner_principal: r.runner_principal,
            status: RunStatus::parse(&r.status),
            created_at: r.created_at,
            evidence_submitted_at: r.evidence_submitted_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_status_machine() {
        use RunStatus::*;
        assert!(Registered.can_transition(Accepted));
        assert!(Registered.can_transition(Rejected));
        assert!(Registered.can_transition(Stale));
        assert!(Registered.can_transition(EvidenceSubmitted));
        assert!(EvidenceSubmitted.can_transition(Accepted));
        // Terminals never move.
        for s in [Accepted, Rejected, Stale] {
            for t in [Registered, EvidenceSubmitted, Accepted, Rejected, Stale] {
                assert!(!s.can_transition(t), "{s:?} -> {t:?} must be illegal");
            }
        }
        assert_eq!(RunStatus::parse("stale").as_str(), "stale");
    }

    #[test]
    fn spec_hash_is_stable() {
        let h = compute_spec_hash(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        assert_eq!(h.len(), 64);
        assert_eq!(
            h,
            compute_spec_hash(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            )
        );
    }

    #[test]
    fn metrics_tol_and_verified_by_n() {
        assert!(metrics_compatible(
            &json!({"acc": 0.9}),
            &json!({"acc": 0.9})
        ));
        assert!(metrics_compatible(
            &json!({"acc": 0.9}),
            &json!({"acc": 0.9 + 1e-10})
        ));
        assert!(!metrics_compatible(
            &json!({"acc": 0.9}),
            &json!({"acc": 0.91})
        ));

        let rows = vec![
            ("a".into(), json!({"acc": 0.9})),
            ("b".into(), json!({"acc": 0.9})),
            ("a".into(), json!({"acc": 0.9})), // same runner twice
            ("c".into(), json!({"acc": 0.5})),
        ];
        let (n, consensus) = compute_verified_by_n(&rows);
        assert_eq!(n, 2); // a and b agree; c does not
        assert_eq!(consensus.unwrap()["acc"], 0.9);
    }
}
