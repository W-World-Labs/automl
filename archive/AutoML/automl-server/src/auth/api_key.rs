// ============================================================
// API Key verification
// ============================================================

use crate::db::Pool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub scholar_id: String,
    pub key_name: String,
    pub trust_level: String,
}

/// Verify a Bearer token against the api_keys table.
/// Token format: aml_xxxx
/// We store sha256(token) in the DB.
pub async fn verify_api_key(pool: &Pool, token: &str) -> Option<ApiKeyInfo> {
    if !token.starts_with("aml_") {
        return None;
    }

    let hash = hash_token(token);

    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT scholar_id, key_name, trust_level FROM api_keys WHERE key_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;

    Some(ApiKeyInfo {
        scholar_id: row.0,
        key_name: row.1,
        trust_level: row.2,
    })
}

pub fn hash_token(token: &str) -> String {
    // Bare lowercase-hex SHA-256 (the stored `key_hash`), byte-identical to the
    // prior `format!("{:x}", Sha256::digest(..))`.
    w_crypto::hash::sha256_hex(token.as_bytes())
}

/// Generate a new API key and store (hashed) in DB.
pub async fn create_api_key(
    pool: &Pool,
    scholar_id: &str,
    key_name: &str,
) -> anyhow::Result<String> {
    let raw = format!("aml_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));

    let hash = w_crypto::hash::sha256_hex(raw.as_bytes());

    sqlx::query(
        "INSERT INTO api_keys (key_hash, scholar_id, key_name, trust_level, created_at, expires_at) \
         VALUES ($1, $2, $3, 'unverified', CURRENT_TIMESTAMP::text, (CURRENT_TIMESTAMP + INTERVAL '365 days')::text)"
    )
    .bind(&hash)
    .bind(scholar_id)
    .bind(key_name)
    .execute(pool)
    .await?;

    Ok(raw)
}

#[cfg(test)]
mod golden {
    //! Byte-identity guard for the w-crypto SHA-256 migration. These vectors were
    //! captured against the prior `format!("{:x}", Sha256::digest(..))` idiom
    //! (independently via `printf %s ... | sha256sum`) and MUST NOT change: the
    //! `key_hash` column and the asset `sha256` / `X-SHA256` wire fields are
    //! persisted/served.
    use super::*;

    #[test]
    fn hash_token_is_byte_identical() {
        assert_eq!(
            hash_token("aml_test_key_123"),
            "640c50f21497366ff566267833e33004d503535b31f168dd5ac3515b889843fc"
        );
    }

    #[test]
    fn asset_sha256_wire_is_byte_identical() {
        // Same primitive backing routes::assets upload (JSON `sha256` + `X-SHA256`).
        assert_eq!(
            w_crypto::hash::sha256_hex(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
