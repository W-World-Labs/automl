//! Silicon scholars (agents) and ed25519 device bindings.

use automl_core::{Page, PageParams};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Agent {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateAgentRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AgentDevice {
    pub id: Uuid,
    pub agent_id: Uuid,
    /// Base64 of the 32-byte ed25519 public key.
    pub pubkey_b64: String,
    pub env_fingerprint: Value,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterDeviceRequest {
    /// Base64 or hex encoding of a 32-byte ed25519 public key.
    pub pubkey: String,
    #[serde(default)]
    pub env_fingerprint: Value,
}

pub async fn create(pool: &PgPool, owner_id: Uuid, req: CreateAgentRequest) -> ApiResult<Agent> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err(ApiError::invalid("name must not be empty"));
    }
    let row = sqlx::query_as::<_, AgentRow>(
        r#"
        INSERT INTO agents (owner_id, name)
        VALUES ($1, $2)
        RETURNING id, owner_id, name, created_at
        "#,
    )
    .bind(owner_id)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(row.into())
}

pub async fn get(pool: &PgPool, id: Uuid) -> ApiResult<Agent> {
    let row = sqlx::query_as::<_, AgentRow>(
        "SELECT id, owner_id, name, created_at FROM agents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("agent not found"))?;
    Ok(row.into())
}

pub async fn list_for_owner(
    pool: &PgPool,
    owner_id: Uuid,
    params: &PageParams,
) -> ApiResult<Page<Agent>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let rows = if let Some(cursor) = params.cursor.as_deref() {
        let (created_at, id) = super::cursor::decode(cursor)?;
        sqlx::query_as::<_, AgentRow>(
            r#"
            SELECT id, owner_id, name, created_at
            FROM agents
            WHERE owner_id = $1 AND (created_at, id) < ($2, $3)
            ORDER BY created_at DESC, id DESC
            LIMIT $4
            "#,
        )
        .bind(owner_id)
        .bind(created_at)
        .bind(id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, AgentRow>(
            r#"
            SELECT id, owner_id, name, created_at
            FROM agents
            WHERE owner_id = $1
            ORDER BY created_at DESC, id DESC
            LIMIT $2
            "#,
        )
        .bind(owner_id)
        .bind(fetch)
        .fetch_all(pool)
        .await?
    };
    Ok(Page::from_rows(
        rows.into_iter().map(Agent::from).collect(),
        limit,
        |a| super::cursor::encode(a.created_at, a.id),
    ))
}

pub async fn register_device(
    pool: &PgPool,
    agent_id: Uuid,
    owner_id: Uuid,
    req: RegisterDeviceRequest,
) -> ApiResult<AgentDevice> {
    ensure_agent_owner(pool, agent_id, owner_id).await?;
    let pubkey = parse_pubkey(&req.pubkey)?;
    let env = if req.env_fingerprint.is_null() {
        Value::Object(Default::default())
    } else if req.env_fingerprint.is_object() {
        req.env_fingerprint
    } else {
        return Err(ApiError::invalid("env_fingerprint must be a JSON object"));
    };

    let row = sqlx::query_as::<_, DeviceRow>(
        r#"
        INSERT INTO agent_devices (agent_id, pubkey, env_fingerprint)
        VALUES ($1, $2, $3)
        RETURNING id, agent_id, pubkey, env_fingerprint, revoked_at, created_at
        "#,
    )
    .bind(agent_id)
    .bind(pubkey.as_slice())
    .bind(&env)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db) = &e {
            if db.constraint() == Some("agent_devices_pubkey_unique") {
                return ApiError::conflict("public key already registered");
            }
        }
        ApiError::from(e)
    })?;
    Ok(row.into())
}

pub async fn revoke_device(
    pool: &PgPool,
    agent_id: Uuid,
    device_id: Uuid,
    owner_id: Uuid,
) -> ApiResult<()> {
    ensure_agent_owner(pool, agent_id, owner_id).await?;
    let res = sqlx::query(
        r#"
        UPDATE agent_devices
        SET revoked_at = now()
        WHERE id = $1 AND agent_id = $2 AND revoked_at IS NULL
        "#,
    )
    .bind(device_id)
    .bind(agent_id)
    .execute(pool)
    .await?;
    if res.rows_affected() > 0 {
        return Ok(());
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_devices WHERE id = $1 AND agent_id = $2)",
    )
    .bind(device_id)
    .bind(agent_id)
    .fetch_one(pool)
    .await?;
    if exists {
        Ok(()) // already revoked
    } else {
        Err(ApiError::not_found("device not found"))
    }
}

pub async fn list_devices(
    pool: &PgPool,
    agent_id: Uuid,
    owner_id: Uuid,
) -> ApiResult<Vec<AgentDevice>> {
    ensure_agent_owner(pool, agent_id, owner_id).await?;
    let rows = sqlx::query_as::<_, DeviceRow>(
        r#"
        SELECT id, agent_id, pubkey, env_fingerprint, revoked_at, created_at
        FROM agent_devices
        WHERE agent_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(AgentDevice::from).collect())
}

/// Load an active (non-revoked) device by id.
pub async fn get_active_device(pool: &PgPool, device_id: Uuid) -> ApiResult<DeviceCrypto> {
    let row = sqlx::query_as::<_, DeviceRow>(
        r#"
        SELECT id, agent_id, pubkey, env_fingerprint, revoked_at, created_at
        FROM agent_devices WHERE id = $1
        "#,
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::signature_invalid("signer device not found"))?;

    if row.revoked_at.is_some() {
        return Err(ApiError::signature_invalid(
            "signer device has been revoked",
        ));
    }
    let pubkey: [u8; 32] = row
        .pubkey
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::signature_invalid("malformed device public key"))?;
    Ok(DeviceCrypto {
        id: row.id,
        agent_id: row.agent_id,
        pubkey,
        env_fingerprint: row.env_fingerprint,
    })
}

#[derive(Debug, Clone)]
pub struct DeviceCrypto {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub pubkey: [u8; 32],
    pub env_fingerprint: Value,
}

impl DeviceCrypto {
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> ApiResult<()> {
        let sig_bytes: [u8; 64] = signature
            .try_into()
            .map_err(|_| ApiError::signature_invalid("signature must be 64 bytes"))?;
        let verifying = VerifyingKey::from_bytes(&self.pubkey)
            .map_err(|_| ApiError::signature_invalid("invalid public key"))?;
        let sig = Signature::from_bytes(&sig_bytes);
        verifying
            .verify(message, &sig)
            .map_err(|_| ApiError::signature_invalid("ed25519 signature verification failed"))
    }
}

pub fn parse_pubkey(raw: &str) -> ApiResult<[u8; 32]> {
    let s = raw.trim();
    let bytes = if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        decode_hex(s)?
    } else {
        B64.decode(s)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s))
            .map_err(|_| ApiError::invalid("pubkey must be base64 or 64-char hex"))?
    };
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| ApiError::invalid("pubkey must be exactly 32 bytes"))
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

async fn ensure_agent_owner(pool: &PgPool, agent_id: Uuid, owner_id: Uuid) -> ApiResult<()> {
    let row: Option<Uuid> = sqlx::query_scalar("SELECT owner_id FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_optional(pool)
        .await?;
    match row {
        None => Err(ApiError::not_found("agent not found")),
        Some(oid) if oid == owner_id => Ok(()),
        Some(_) => Err(ApiError::forbidden("not the agent owner")),
    }
}

#[derive(sqlx::FromRow)]
struct AgentRow {
    id: Uuid,
    owner_id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
}

impl From<AgentRow> for Agent {
    fn from(r: AgentRow) -> Self {
        Self {
            id: r.id,
            owner_id: r.owner_id,
            name: r.name,
            created_at: r.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DeviceRow {
    id: Uuid,
    agent_id: Uuid,
    pubkey: Vec<u8>,
    env_fingerprint: Value,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<DeviceRow> for AgentDevice {
    fn from(r: DeviceRow) -> Self {
        Self {
            id: r.id,
            agent_id: r.agent_id,
            pubkey_b64: B64.encode(&r.pubkey),
            env_fingerprint: r.env_fingerprint,
            revoked_at: r.revoked_at,
            created_at: r.created_at,
        }
    }
}

/// Canonical message bytes for evidence signing — delegates to the shared
/// wire-contract implementation in `automl-core` (the CLI signs the same bytes).
pub fn canonical_evidence_message(
    run_id: Uuid,
    exit_code: i32,
    logs_object_key: &Option<String>,
    artifact_hashes: &Value,
    metrics: &Value,
    env_fingerprint: &Value,
) -> Vec<u8> {
    automl_core::canonical_evidence_message(
        &run_id.to_string(),
        exit_code,
        logs_object_key.as_deref(),
        artifact_hashes,
        metrics,
        env_fingerprint,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    #[test]
    fn verify_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let pk = sk.verifying_key();
        let msg = b"hello automl";
        let sig = sk.sign(msg);
        let device = DeviceCrypto {
            id: Uuid::nil(),
            agent_id: Uuid::nil(),
            pubkey: pk.to_bytes(),
            env_fingerprint: Value::Object(Default::default()),
        };
        device.verify(msg, &sig.to_bytes()).unwrap();
        assert!(device.verify(b"tampered", &sig.to_bytes()).is_err());
    }

    #[test]
    fn parse_pubkey_hex_and_b64() {
        let sk = SigningKey::generate(&mut OsRng);
        let bytes = sk.verifying_key().to_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(parse_pubkey(&hex).unwrap(), bytes);
        assert_eq!(parse_pubkey(&B64.encode(bytes)).unwrap(), bytes);
    }
}
