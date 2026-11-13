//! CLI loopback login: one-time codes bridging a browser W-ID session to a
//! freshly minted API key.
//!
//! Flow: `automl login` starts a loopback listener, generates a PKCE verifier
//! and opens the browser at `GET /api/v1/cli/auth/start?redirect_uri=…&
//! challenge=hex(sha256(verifier))`. That page requires a W-ID session (302 to
//! `/auth/login?next=…` otherwise), then issues a single-use short-TTL code and
//! redirects to the loopback. The CLI exchanges code+verifier at
//! `POST /api/v1/cli/auth/token` for an `amk_*` key. Codes are stored hashed;
//! verification is constant-time.

use automl_core::scope;
use chrono::{Duration, Utc};
use rand::RngCore;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::keys::{self, ApiKeyCreated, CreateKeyRequest},
    error::{ApiError, ApiResult},
};

pub const CODE_TTL_MINUTES: i64 = 5;

/// Loopback redirect targets only: `http://127.0.0.1:<port>/callback`.
pub fn validate_redirect_uri(uri: &str) -> ApiResult<()> {
    let rest = uri.strip_prefix("http://127.0.0.1:").ok_or_else(|| {
        ApiError::invalid("redirect_uri must be http://127.0.0.1:<port>/callback")
    })?;
    let (port, path) = rest
        .split_once('/')
        .ok_or_else(|| ApiError::invalid("redirect_uri must include /callback path"))?;
    if port.parse::<u16>().is_err() || path != "callback" {
        return Err(ApiError::invalid(
            "redirect_uri must be http://127.0.0.1:<port>/callback",
        ));
    }
    Ok(())
}

pub fn validate_challenge(challenge: &str) -> ApiResult<()> {
    if challenge.len() != 64 || !challenge.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::invalid(
            "challenge must be hex(sha256(verifier)) — 64 hex chars",
        ));
    }
    Ok(())
}

/// Issue a one-time code for an authenticated scholar. Returns the plaintext
/// code (only ever sent through the loopback redirect).
pub async fn issue_code(
    db: &PgPool,
    scholar_id: Uuid,
    challenge: &str,
    redirect_uri: &str,
) -> ApiResult<String> {
    validate_challenge(challenge)?;
    validate_redirect_uri(redirect_uri)?;
    let mut raw = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut raw);
    let code = w_crypto::hex_encode(&raw);
    let code_hash = w_crypto::sha256_hex(code.as_bytes());
    sqlx::query(
        r#"
        INSERT INTO cli_auth_codes (code_hash, scholar_id, challenge, redirect_uri, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(&code_hash)
    .bind(scholar_id)
    .bind(challenge.to_ascii_lowercase())
    .bind(redirect_uri)
    .bind(Utc::now() + Duration::minutes(CODE_TTL_MINUTES))
    .execute(db)
    .await?;
    Ok(code)
}

/// Exchange code + verifier for a new API key. Single-use is enforced
/// atomically; the minted key carries the scholar default scopes (never admin).
pub async fn exchange(
    db: &PgPool,
    code: &str,
    verifier: &str,
    key_name: &str,
) -> ApiResult<ApiKeyCreated> {
    let code_hash = w_crypto::sha256_hex(code.as_bytes());
    // Claim the code atomically — a concurrent replay loses the race.
    let row: Option<(Uuid, String)> = sqlx::query_as(
        r#"
        UPDATE cli_auth_codes SET used_at = now()
        WHERE code_hash = $1 AND used_at IS NULL AND expires_at > now()
        RETURNING scholar_id, challenge
        "#,
    )
    .bind(&code_hash)
    .fetch_optional(db)
    .await?;
    let (scholar_id, challenge) =
        row.ok_or_else(|| ApiError::unauthorized("invalid, used, or expired authorization code"))?;

    let derived = w_crypto::sha256_hex(verifier.as_bytes());
    if !w_crypto::ct_eq(derived.as_bytes(), challenge.trim().as_bytes()) {
        return Err(ApiError::unauthorized("PKCE verifier mismatch"));
    }

    let scopes: Vec<String> = scope::SCHOLAR_DEFAULT
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let name = if key_name.trim().is_empty() {
        "cli".to_string()
    } else {
        key_name.trim().to_string()
    };
    keys::create_key(
        db,
        scholar_id,
        CreateKeyRequest {
            name,
            scopes: scopes.clone(),
            expires_at: None,
        },
        &scopes,
        false,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uri_is_loopback_only() {
        assert!(validate_redirect_uri("http://127.0.0.1:8123/callback").is_ok());
        assert!(validate_redirect_uri("http://127.0.0.1:0/callback").is_ok());
        assert!(validate_redirect_uri("http://localhost:8123/callback").is_err());
        assert!(validate_redirect_uri("https://127.0.0.1:8123/callback").is_err());
        assert!(validate_redirect_uri("http://127.0.0.1:8123/evil").is_err());
        assert!(validate_redirect_uri("http://127.0.0.1:8123/callback/x").is_err());
        assert!(validate_redirect_uri("http://evil.test/callback").is_err());
    }

    #[test]
    fn challenge_must_be_hex64() {
        assert!(validate_challenge(&"a".repeat(64)).is_ok());
        assert!(validate_challenge(&"a".repeat(63)).is_err());
        assert!(validate_challenge(&"g".repeat(64)).is_err());
    }
}
