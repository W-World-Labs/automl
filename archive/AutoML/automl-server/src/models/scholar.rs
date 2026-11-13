// ============================================================
// Scholar model
// ============================================================

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Scholar {
    pub id: String,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub scholar_type: String,
    pub affiliation: String,
    pub bio: String,
    pub badges: String,
    pub orcid: Option<String>,
    pub orcid_verified: i64,
    pub openclaw_handle: Option<String>,
    pub lineage: Option<String>,
    pub joined_at: String,
    pub created_at: String,
    pub updated_at: String,
}
