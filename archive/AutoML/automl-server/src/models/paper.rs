// ============================================================
// Paper model
// ============================================================

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Paper status — all values used across the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaperStatus {
    Submitted,
    ReproducibilityGate,
    UnderReview,
    Rebuttal,
    Accepted,
    Rejected,
    DeskRejected,
    Published,
    Retracted,
    Corrected,
}

impl PaperStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submitted => "SUBMITTED",
            Self::ReproducibilityGate => "REPRODUCIBILITY_GATE",
            Self::UnderReview => "UNDER_REVIEW",
            Self::Rebuttal => "REBUTTAL",
            Self::Accepted => "ACCEPTED",
            Self::Rejected => "REJECTED",
            Self::DeskRejected => "DESK_REJECTED",
            Self::Published => "PUBLISHED",
            Self::Retracted => "RETRACTED",
            Self::Corrected => "CORRECTED",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "SUBMITTED" => Some(Self::Submitted),
            "REPRODUCIBILITY_GATE" => Some(Self::ReproducibilityGate),
            "UNDER_REVIEW" => Some(Self::UnderReview),
            "REBUTTAL" => Some(Self::Rebuttal),
            "ACCEPTED" => Some(Self::Accepted),
            "REJECTED" => Some(Self::Rejected),
            "DESK_REJECTED" => Some(Self::DeskRejected),
            "PUBLISHED" => Some(Self::Published),
            "RETRACTED" => Some(Self::Retracted),
            "CORRECTED" => Some(Self::Corrected),
            _ => None,
        }
    }

    /// Valid next states from current state
    pub fn valid_transitions(&self) -> &[PaperStatus] {
        match self {
            Self::Submitted => &[Self::ReproducibilityGate, Self::DeskRejected],
            Self::ReproducibilityGate => &[Self::UnderReview, Self::DeskRejected],
            Self::UnderReview => &[Self::Rebuttal, Self::Accepted, Self::Rejected],
            Self::Rebuttal => &[Self::Accepted, Self::Rejected],
            Self::Accepted => &[Self::Published, Self::Retracted],
            Self::Published => &[Self::Retracted, Self::Corrected],
            _ => &[], // Terminal states
        }
    }
}

/// Valid journal slugs
pub const VALID_JOURNALS: &[&str] = &[
    "kindergarten",
    "auto",
    "intelligence",
    "science",
    "life",
    "engineering",
    "society",
    "humanities",
];

/// Humanities journals — exempt from reproducibility requirements
pub const HUMANITIES_JOURNALS: &[&str] = &["society", "humanities"];

/// Paper record from DB
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Paper {
    pub id: String,
    pub title: String,
    #[sqlx(rename = "abstract")]
    #[serde(rename = "abstract")]
    pub paper_abstract: String,
    pub domain: String,
    pub journal_slug: String,
    pub authors: String, // JSON string
    pub status: String,
    pub compute_tier: String,
    pub submitted_at: String,
    pub citations: i64,
    pub repro_score: i64,
    pub pdf_key: Option<String>,
    pub pdf_url: Option<String>,
    pub github_repo: Option<String>,
    pub owner_user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
