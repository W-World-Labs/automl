//! Dedicated Gitea integration: per-paper repo provisioning + push webhook sync.
//!
//! The platform never stores code — papers hold `repo_url + commit` pointers and
//! the code body lives on the dedicated Gitea instance (`deploy/gitea/`). This
//! module is the only place that talks to it. When the `GITEA_*` env contract is
//! absent the whole integration is disabled: `AppState.gitea` is `None` and the
//! webhook endpoint answers 501.

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::GiteaConfig;

/// Outcome of applying an inbound webhook event.
#[derive(Debug, PartialEq, Eq)]
pub enum EventOutcome {
    /// Push applied to a known paper.
    Applied { paper_id: Uuid },
    /// Event type or target repo is not ours — acknowledged and dropped.
    Ignored,
}

pub struct GiteaClient {
    cfg: GiteaConfig,
    http: reqwest::Client,
}

impl GiteaClient {
    pub fn new(cfg: GiteaConfig) -> Self {
        Self {
            cfg,
            http: w_http::client(),
        }
    }

    pub fn org(&self) -> &str {
        &self.cfg.org
    }

    /// Clone URL for a paper repo on the dedicated instance.
    pub fn repo_url(&self, name: &str) -> String {
        format!(
            "{}/{}/{}.git",
            self.cfg.url.trim_end_matches('/'),
            self.cfg.org,
            name
        )
    }

    /// Verify the `X-Gitea-Signature` header (hex HMAC-SHA256 of the raw body).
    /// Constant-time comparison via `w_crypto::hmac_sha256_verify`.
    pub fn verify_signature(&self, body: &[u8], signature_hex: &str) -> bool {
        let Ok(tag) = w_crypto::hex_decode(signature_hex.trim()) else {
            return false;
        };
        w_crypto::hmac_sha256_verify(self.cfg.webhook_secret.as_bytes(), body, &tag)
    }

    /// Create the org repo if it does not exist. Idempotent: 201 and 409 both
    /// succeed. Retries transient failures (w-http policy) before giving up.
    pub async fn ensure_repo(&self, name: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/api/v1/orgs/{}/repos",
            self.cfg.url.trim_end_matches('/'),
            self.cfg.org
        );
        let body = serde_json::json!({
            "name": name,
            "private": false,
            "auto_init": false,
            "description": format!("Auto.ml paper {name}"),
        });
        let resp = w_http::send_with_retry(
            || {
                self.http
                    .post(&url)
                    .header("Authorization", format!("token {}", self.cfg.token))
                    .json(&body)
            },
            &w_http::RetryPolicy::default(),
        )
        .await?;
        match resp.status().as_u16() {
            201 | 409 => Ok(()),
            other => anyhow::bail!("gitea create repo {name}: unexpected status {other}"),
        }
    }
}

/// Provision the Gitea repo for one `repo_pending` paper, then mark it
/// `repo_ready` and fill `repo_url` if the paper does not point elsewhere.
pub async fn provision_paper(
    db: &PgPool,
    client: &GiteaClient,
    paper_id: Uuid,
) -> anyhow::Result<()> {
    let name = paper_id.to_string();
    client.ensure_repo(&name).await?;
    sqlx::query(
        r#"
        UPDATE papers SET
            repo_url = CASE WHEN repo_url = '' THEN $2 ELSE repo_url END,
            code_status = 'repo_ready',
            updated_at = now()
        WHERE id = $1 AND code_status = 'repo_pending'
        "#,
    )
    .bind(paper_id)
    .bind(client.repo_url(&name))
    .execute(db)
    .await?;
    Ok(())
}

/// Compensation sweep driven by `w_task::Periodic`: retry every paper still in
/// `repo_pending` (creation-time attempt failed or the server restarted).
/// Failures are logged and retried on the next tick — never fatal.
pub async fn provision_pending(db: &PgPool, client: &GiteaClient) -> anyhow::Result<()> {
    let ids: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM papers WHERE code_status = 'repo_pending' ORDER BY updated_at LIMIT 20",
    )
    .fetch_all(db)
    .await?;
    for (id,) in ids {
        if let Err(err) = provision_paper(db, client, id).await {
            tracing::warn!(paper_id = %id, error = %err, "gitea repo provisioning failed; will retry");
        }
    }
    Ok(())
}

/// Apply a verified webhook event. Only `push` mutates state: the repo name is
/// the paper id, `after` becomes `latest_commit`, `code_status` → `ready`.
/// Anything unrecognized is acknowledged and ignored (204 at the HTTP layer).
pub async fn apply_event(
    db: &PgPool,
    event_type: &str,
    payload: &Value,
) -> anyhow::Result<EventOutcome> {
    if event_type != "push" {
        return Ok(EventOutcome::Ignored);
    }
    let Some(repo_name) = payload.pointer("/repository/name").and_then(Value::as_str) else {
        return Ok(EventOutcome::Ignored);
    };
    let Ok(paper_id) = Uuid::parse_str(repo_name) else {
        return Ok(EventOutcome::Ignored);
    };
    let Some(after) = payload.get("after").and_then(Value::as_str) else {
        return Ok(EventOutcome::Ignored);
    };
    let after = after.to_ascii_lowercase();
    if after.len() != 40 || !after.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(EventOutcome::Ignored);
    }

    let updated = sqlx::query(
        r#"
        UPDATE papers SET
            latest_commit = $2,
            code_status = 'ready',
            updated_at = now()
        WHERE id = $1
          -- state machine: only the platform-hosted chain reaches `ready`;
          -- an external pointer (`unknown`) is never webhook-addressable.
          AND code_status IN ('repo_pending', 'repo_ready', 'ready')
        "#,
    )
    .bind(paper_id)
    .bind(&after)
    .execute(db)
    .await?
    .rows_affected();

    if updated == 0 {
        return Ok(EventOutcome::Ignored);
    }
    Ok(EventOutcome::Applied { paper_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> GiteaClient {
        GiteaClient::new(GiteaConfig {
            url: "https://code.example.test/".into(),
            token: "t".into(),
            org: "papers".into(),
            webhook_secret: "s3cret".into(),
        })
    }

    #[test]
    fn repo_url_shape() {
        assert_eq!(
            client().repo_url("abc"),
            "https://code.example.test/papers/abc.git"
        );
    }

    #[test]
    fn signature_verifies_and_rejects() {
        let c = client();
        let body = b"{\"x\":1}";
        let good = w_crypto::hmac_sha256_hex(b"s3cret", body);
        assert!(c.verify_signature(body, &good));
        assert!(!c.verify_signature(body, &good.replace('a', "b")));
        assert!(!c.verify_signature(body, "not-hex"));
        assert!(!c.verify_signature(b"tampered", &good));
    }
}
