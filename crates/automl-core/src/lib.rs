//! Shared API contract primitives used by both automl-server and automl-cli.
//!
//! Everything here is wire format: once a code or field ships it is stable and
//! must never be renamed, only added to.

use serde::{Deserialize, Serialize};

/// Machine-readable error codes carried in every error envelope:
/// `{"error": {"code": "...", "message": "..."}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    PayloadTooLarge,
    RateLimited,
    /// Evidence signature missing/invalid or device revoked (iteration 4).
    SignatureInvalid,
    Unavailable,
    /// Feature exists in the API surface but is not configured/implemented
    /// on this deployment (stub-is-501 rule).
    NotImplemented,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::RateLimited => "RATE_LIMITED",
            Self::SignatureInvalid => "SIGNATURE_INVALID",
            Self::Unavailable => "UNAVAILABLE",
            Self::NotImplemented => "NOT_IMPLEMENTED",
            Self::Internal => "INTERNAL",
        }
    }
}

// ── Scopes (wire contract — only add, never rename) ─────────────────────────

/// Permission scopes carried on sessions and API keys.
///
/// Replaces v1's free-form `trust_level` string. Values are stable wire tokens.
pub mod scope {
    pub const PAPERS_READ: &str = "papers:read";
    pub const PAPERS_WRITE: &str = "papers:write";
    pub const REVIEWS_WRITE: &str = "reviews:write";
    pub const REPRODUCE_SUBMIT: &str = "reproduce:submit";
    pub const ASSETS_WRITE: &str = "assets:write";
    pub const KEYS_MANAGE: &str = "keys:manage";
    pub const FORUM_WRITE: &str = "forum:write";
    /// Research Hub projects (docs/RESEARCH-HUB-DESIGN.md §12.1). `admin` is
    /// the project-level token (lifecycle/ACL), not platform admin.
    pub const PROJECTS_READ: &str = "projects:read";
    pub const PROJECTS_WRITE: &str = "projects:write";
    pub const PROJECTS_ADMIN: &str = "projects:admin";
    pub const ADMIN: &str = "admin";

    /// Complete initial set. Order is stable for docs/OpenAPI examples only.
    pub const ALL: &[&str] = &[
        PAPERS_READ,
        PAPERS_WRITE,
        REVIEWS_WRITE,
        REPRODUCE_SUBMIT,
        ASSETS_WRITE,
        KEYS_MANAGE,
        FORUM_WRITE,
        PROJECTS_READ,
        PROJECTS_WRITE,
        PROJECTS_ADMIN,
        ADMIN,
    ];

    /// Scopes granted to a normal W-ID session (everything except platform admin).
    pub const SCHOLAR_DEFAULT: &[&str] = &[
        PAPERS_READ,
        PAPERS_WRITE,
        REVIEWS_WRITE,
        REPRODUCE_SUBMIT,
        ASSETS_WRITE,
        KEYS_MANAGE,
        FORUM_WRITE,
        PROJECTS_READ,
        PROJECTS_WRITE,
        PROJECTS_ADMIN,
    ];

    pub fn is_known(s: &str) -> bool {
        ALL.contains(&s)
    }
}

// ── Outbound webhook event types (wire contract — only add, never rename) ───

/// Event types delivered to webhook subscriptions. Payload envelope:
/// `{"event_id", "event_type", "occurred_at", "data"}`, signed with
/// `X-Automl-Signature: sha256=<HMAC-SHA256(secret, body)>`.
pub mod event {
    pub const PAPER_DECIDED: &str = "paper.decided";
    pub const REPRO_COMPLETED: &str = "repro.completed";

    pub const ALL: &[&str] = &[PAPER_DECIDED, REPRO_COMPLETED];

    pub fn is_known(s: &str) -> bool {
        ALL.contains(&s)
    }
}

/// Canonical message bytes for reproduce-evidence signing. CLI signs these
/// bytes with the device key; the server verifies against the registered
/// pubkey. Field order is fixed (serde_json's default map is sorted) — both
/// sides MUST build the message through this one function.
pub fn canonical_evidence_message(
    run_id: &str,
    exit_code: i32,
    logs_object_key: Option<&str>,
    artifact_hashes: &serde_json::Value,
    metrics: &serde_json::Value,
    env_fingerprint: &serde_json::Value,
) -> Vec<u8> {
    let v = serde_json::json!({
        "artifact_hashes": artifact_hashes,
        "env_fingerprint": env_fingerprint,
        "exit_code": exit_code,
        "logs_object_key": logs_object_key,
        "metrics": metrics,
        "run_id": run_id,
    });
    serde_json::to_vec(&v).expect("canonical evidence serializes")
}

pub const DEFAULT_PAGE_LIMIT: u32 = 50;
pub const MAX_PAGE_LIMIT: u32 = 200;

/// Query-string pagination parameters accepted by every list endpoint.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct PageParams {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

impl PageParams {
    /// Effective per-page limit, clamped to `1..=MAX_PAGE_LIMIT`.
    pub fn limit(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_PAGE_LIMIT)
            .clamp(1, MAX_PAGE_LIMIT)
    }
}

/// Uniform list envelope: `{"data": [...], "next_cursor": "..."}`.
/// `next_cursor = null` means the listing is exhausted.
#[derive(Debug, Serialize, Deserialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Build a page from one-more-than-limit query results: if `rows` exceeds
    /// `limit`, the extra row is dropped and `next_cursor` is produced from the
    /// last kept row via `cursor_of`.
    pub fn from_rows(mut rows: Vec<T>, limit: u32, cursor_of: impl Fn(&T) -> String) -> Self {
        let has_more = rows.len() as u32 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }
        let next_cursor = has_more.then(|| rows.last().map(&cursor_of)).flatten();
        Self {
            data: rows,
            next_cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_params_clamp() {
        assert_eq!(PageParams::default().limit(), DEFAULT_PAGE_LIMIT);
        assert_eq!(
            PageParams {
                limit: Some(0),
                cursor: None
            }
            .limit(),
            1
        );
        assert_eq!(
            PageParams {
                limit: Some(9999),
                cursor: None
            }
            .limit(),
            MAX_PAGE_LIMIT
        );
    }

    #[test]
    fn page_from_rows_sets_cursor_only_when_more() {
        let page = Page::from_rows(vec![1, 2, 3], 2, |n| n.to_string());
        assert_eq!(page.data, vec![1, 2]);
        assert_eq!(page.next_cursor.as_deref(), Some("2"));

        let page = Page::from_rows(vec![1, 2], 2, |n| n.to_string());
        assert_eq!(page.data, vec![1, 2]);
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn error_code_wire_format_is_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::NotFound).unwrap(),
            "\"NOT_FOUND\""
        );
        assert_eq!(ErrorCode::RateLimited.as_str(), "RATE_LIMITED");
        assert_eq!(
            serde_json::to_string(&ErrorCode::SignatureInvalid).unwrap(),
            "\"SIGNATURE_INVALID\""
        );
        assert_eq!(ErrorCode::SignatureInvalid.as_str(), "SIGNATURE_INVALID");
    }

    #[test]
    fn scope_set_is_stable() {
        assert!(scope::is_known("papers:read"));
        assert!(scope::is_known("admin"));
        assert!(!scope::is_known("trust_level"));
        assert!(!scope::is_known("papers:delete"));
        assert_eq!(scope::ALL.len(), 11);
        assert!(scope::is_known("forum:write"));
        assert!(scope::is_known("projects:read"));
        assert!(scope::is_known("projects:write"));
        assert!(scope::is_known("projects:admin"));
        assert!(!scope::SCHOLAR_DEFAULT.contains(&scope::ADMIN));
        assert!(scope::SCHOLAR_DEFAULT.contains(&scope::FORUM_WRITE));
    }

    #[test]
    fn event_types_are_known() {
        assert!(event::is_known("paper.decided"));
        assert!(event::is_known("repro.completed"));
        assert!(!event::is_known("paper.created"));
    }
}
