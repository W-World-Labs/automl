//! Client compute profiles: self-reported environment descriptors.
//!
//! Provenance only. Evidence `env_fingerprint`s reference a `profile_hash` so
//! readers can judge whether identical results came from heterogeneous
//! environments. There is no scheduling semantics here whatsoever.

use automl_core::{Page, PageParams};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ComputeProfile {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub profile: Value,
    pub profile_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateProfileRequest {
    /// Attribute the environment to one of the caller's agents (optional).
    pub agent_id: Option<Uuid>,
    /// Free-form environment descriptor: gpu, os, cpu, deps fingerprint, …
    pub profile: Value,
}

#[derive(Debug, Default, Deserialize)]
pub struct ProfileFilter {
    pub agent_id: Option<Uuid>,
}

/// Content-address the profile: sha256 over a canonical (recursively
/// key-sorted) JSON encoding, so key order in the request never changes the hash.
pub fn profile_hash(profile: &Value) -> String {
    w_crypto::sha256_hex(canonical_json(profile).as_bytes())
}

fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).expect("string serializes"),
                        canonical_json(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", inner.join(","))
        }
        other => serde_json::to_string(other).expect("scalar serializes"),
    }
}

/// Register a profile. Idempotent per owner: identical content (same canonical
/// hash) returns the existing row — `created` is false in that case.
pub async fn create(
    db: &PgPool,
    owner: Uuid,
    req: CreateProfileRequest,
) -> ApiResult<(ComputeProfile, bool)> {
    if !req.profile.is_object() {
        return Err(ApiError::invalid("profile must be a JSON object"));
    }
    if let Some(agent_id) = req.agent_id {
        let owned: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM agents WHERE id = $1 AND owner_id = $2")
                .bind(agent_id)
                .bind(owner)
                .fetch_optional(db)
                .await?;
        if owned.is_none() {
            return Err(ApiError::invalid("agent_id does not belong to caller"));
        }
    }

    let hash = profile_hash(&req.profile);
    let row = sqlx::query_as::<_, ProfileRow>(
        r#"
        INSERT INTO compute_profiles (owner_id, agent_id, profile, profile_hash)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (owner_id, profile_hash) DO NOTHING
        RETURNING id, owner_id, agent_id, profile, profile_hash, created_at
        "#,
    )
    .bind(owner)
    .bind(req.agent_id)
    .bind(&req.profile)
    .bind(&hash)
    .fetch_optional(db)
    .await?;

    if let Some(row) = row {
        return Ok((row.into(), true));
    }
    // Conflict path: reuse the existing identical profile.
    let existing = sqlx::query_as::<_, ProfileRow>(
        r#"
        SELECT id, owner_id, agent_id, profile, profile_hash, created_at
        FROM compute_profiles WHERE owner_id = $1 AND profile_hash = $2
        "#,
    )
    .bind(owner)
    .bind(&hash)
    .fetch_one(db)
    .await?;
    Ok((existing.into(), false))
}

pub async fn list(
    db: &PgPool,
    params: &PageParams,
    filter: &ProfileFilter,
) -> ApiResult<Page<ComputeProfile>> {
    let limit = params.limit();
    let fetch = limit as i64 + 1;
    let (after_ts, after_id) = match params.cursor.as_deref() {
        Some(c) => {
            let (ts, id) = super::cursor::decode(c)?;
            (Some(ts), Some(id))
        }
        None => (None, None),
    };

    let rows = sqlx::query_as::<_, ProfileRow>(
        r#"
        SELECT id, owner_id, agent_id, profile, profile_hash, created_at
        FROM compute_profiles
        WHERE ($1::uuid IS NULL OR agent_id = $1)
          AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(filter.agent_id)
    .bind(after_ts)
    .bind(after_id)
    .bind(fetch)
    .fetch_all(db)
    .await?;

    Ok(Page::from_rows(
        rows.into_iter().map(ComputeProfile::from).collect(),
        limit,
        |p| super::cursor::encode(p.created_at, p.id),
    ))
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: Uuid,
    owner_id: Uuid,
    agent_id: Option<Uuid>,
    profile: Value,
    profile_hash: String,
    created_at: DateTime<Utc>,
}

impl From<ProfileRow> for ComputeProfile {
    fn from(r: ProfileRow) -> Self {
        Self {
            id: r.id,
            owner_id: r.owner_id,
            agent_id: r.agent_id,
            profile: r.profile,
            profile_hash: r.profile_hash.trim().to_string(),
            created_at: r.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_key_order_independent() {
        let a = json!({"gpu": "H100", "os": {"name": "linux", "kernel": "6.8"}});
        let b = json!({"os": {"kernel": "6.8", "name": "linux"}, "gpu": "H100"});
        assert_eq!(profile_hash(&a), profile_hash(&b));
        let c = json!({"gpu": "H200", "os": {"name": "linux", "kernel": "6.8"}});
        assert_ne!(profile_hash(&a), profile_hash(&c));
    }

    #[test]
    fn hash_distinguishes_array_order() {
        let a = json!({"deps": ["torch", "numpy"]});
        let b = json!({"deps": ["numpy", "torch"]});
        assert_ne!(profile_hash(&a), profile_hash(&b));
    }
}
