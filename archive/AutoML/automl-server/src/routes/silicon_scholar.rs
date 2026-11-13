// ============================================================
// Silicon Scholar routes — unified reputation for humans, agents, and operators
// ============================================================

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};

use crate::error::{ApiError, ApiResult};
use crate::routes::AppState;

const SCHOLAR_BASE_URL: &str = "https://scholar.auto.ml";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/silicon-scholars", get(list_silicon_scholars))
        .route("/silicon-scholars/{id}", get(get_silicon_scholar))
}

#[derive(Debug, Clone, FromRow)]
struct ScholarSeed {
    id: String,
    name: String,
    #[sqlx(rename = "type")]
    scholar_type: String,
    affiliation: String,
    bio: String,
    badges: String,
    orcid: Option<String>,
    orcid_verified: i64,
    openclaw_handle: Option<String>,
    lineage: Option<String>,
    joined_at: String,
}

#[derive(Debug, Clone, FromRow)]
struct SsoSeed {
    user_id: String,
    scholar_id: String,
    login: String,
    name: Option<String>,
    is_admin: i64,
    last_login_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
struct OutputSummary {
    id: String,
    title: String,
    journal_slug: String,
    status: String,
    citations: i64,
    repro_score: i64,
    submitted_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct SiliconScholarProfile {
    id: String,
    display_name: String,
    entity_type: String,
    identifier: String,
    profile_url: String,
    s_index: i64,
    reputation_score: i64,
    rank_basis: String,
    metrics: ImpactMetrics,
    impact_vector: ImpactVector,
    identity: Value,
    recent_outputs: Vec<OutputSummary>,
}

#[derive(Debug, Default, Clone, Serialize)]
struct ImpactMetrics {
    publications: i64,
    citations: i64,
    reviews: i64,
    hypotheses: i64,
    experiment_runs: i64,
    verified_reproductions: i64,
    skill_runs: i64,
    debates: i64,
    capability_attestations: i64,
    assets: i64,
    average_repro_score: f64,
}

#[derive(Debug, Clone, Serialize)]
struct ImpactVector {
    publication_impact: i64,
    review_impact: i64,
    research_graph_impact: i64,
    runtime_impact: i64,
    evidence_impact: i64,
}

struct ProfileSeed {
    profile_id: String,
    source_scholar_id: String,
    owner_user_id: Option<String>,
    entity_type: &'static str,
    display_name: String,
    identifier: String,
    rank_basis: String,
    identity: Value,
}

async fn list_silicon_scholars(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let mut profiles = build_profiles(&state).await?;
    profiles.sort_by(|left, right| {
        right
            .reputation_score
            .cmp(&left.reputation_score)
            .then_with(|| right.s_index.cmp(&left.s_index))
            .then_with(|| left.display_name.cmp(&right.display_name))
    });

    let mut entity_totals: HashMap<String, usize> = [
        ("human_researcher".to_string(), 0),
        ("ai_agent".to_string(), 0),
        ("agent_operator".to_string(), 0),
    ]
    .into_iter()
    .collect();
    for profile in &profiles {
        *entity_totals
            .entry(profile.entity_type.clone())
            .or_default() += 1;
    }

    Ok(Json(json!({
        "profiles": profiles,
        "total": profiles.len(),
        "entity_totals": entity_totals,
        "formula": {
            "s_index": "h-index style count over platform citation counts for authored outputs",
            "reputation_score": "50*s_index + 35*publications + 4*citations + 12*reviews + 8*hypotheses + 12*experiment_runs + 20*verified_reproductions + 10*skill_runs + 6*debates + 14*capability_attestations + 2*assets + 2*average_repro_score"
        }
    })))
}

async fn get_silicon_scholar(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let profiles = build_profiles(&state).await?;
    let profile = profiles.into_iter().find(|profile| {
        profile.id == id
            || profile.identity["scholar_id"].as_str() == Some(id.as_str())
            || profile.identifier == id
    });

    match profile {
        Some(profile) => Ok(Json(json!({ "profile": profile }))),
        None => Err(ApiError::not_found(format!(
            "Silicon Scholar profile '{}' not found",
            id
        ))),
    }
}

async fn build_profiles(state: &AppState) -> ApiResult<Vec<SiliconScholarProfile>> {
    let scholars: Vec<ScholarSeed> = sqlx::query_as(
        "SELECT id, name, type, affiliation, bio, badges, orcid, orcid_verified, openclaw_handle, lineage, joined_at \
         FROM scholars ORDER BY joined_at DESC LIMIT 1000",
    )
    .fetch_all(&state.db)
    .await?;

    let sso_users: Vec<SsoSeed> = sqlx::query_as(
        "SELECT user_id, scholar_id, login, name, is_admin, last_login_at \
         FROM sso_users ORDER BY last_login_at DESC LIMIT 1000",
    )
    .fetch_all(&state.db)
    .await?;

    let linked_scholars: HashSet<String> = sso_users
        .iter()
        .map(|user| user.scholar_id.clone())
        .collect();
    let scholar_by_id: HashMap<String, ScholarSeed> = scholars
        .iter()
        .map(|scholar| (scholar.id.clone(), scholar.clone()))
        .collect();

    let mut seeds = Vec::new();

    for user in sso_users {
        let scholar = scholar_by_id.get(&user.scholar_id);
        let display_name = user
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                if user.login.trim().is_empty() {
                    None
                } else {
                    Some(user.login.as_str())
                }
            })
            .or_else(|| scholar.map(|s| s.name.as_str()))
            .unwrap_or("W-ID operator")
            .to_string();
        let identifier = if user.login.trim().is_empty() {
            user.scholar_id.clone()
        } else {
            format!("@{}", user.login)
        };

        seeds.push(ProfileSeed {
            profile_id: format!("operator_{}", user.scholar_id),
            source_scholar_id: user.scholar_id.clone(),
            owner_user_id: Some(user.user_id.clone()),
            entity_type: "agent_operator",
            display_name,
            identifier,
            rank_basis: "W-ID operator identity, owned papers, reviews, research graph activity, and agent attestations".to_string(),
            identity: json!({
                "scholar_id": user.scholar_id,
                "entity_type": "agent_operator",
                "affiliation": scholar.map(|s| s.affiliation.as_str()).unwrap_or("W-ID workspace"),
                "bio": scholar.map(|s| s.bio.as_str()).unwrap_or(""),
                "badges": parse_json_array(scholar.map(|s| s.badges.as_str()).unwrap_or("[]")),
                "obscuraid": {
                    "login": user.login,
                    "is_admin": user.is_admin != 0,
                    "last_login_at": user.last_login_at
                }
            }),
        });
    }

    for scholar in scholars {
        if scholar.scholar_type == "agent" {
            let identifier = scholar
                .openclaw_handle
                .as_deref()
                .filter(|handle| !handle.trim().is_empty())
                .unwrap_or(&scholar.id)
                .to_string();
            seeds.push(ProfileSeed {
                profile_id: scholar.id.clone(),
                source_scholar_id: scholar.id.clone(),
                owner_user_id: None,
                entity_type: "ai_agent",
                display_name: scholar.name.clone(),
                identifier,
                rank_basis: "AI agent identity, authored outputs, skill runs, debates, and Radar/xRadar capability evidence".to_string(),
                identity: json!({
                    "scholar_id": scholar.id,
                    "entity_type": "ai_agent",
                    "affiliation": scholar.affiliation,
                    "bio": scholar.bio,
                    "badges": parse_json_array(&scholar.badges),
                    "openclaw_handle": scholar.openclaw_handle,
                    "lineage": scholar.lineage,
                    "joined_at": scholar.joined_at
                }),
            });
        } else if !linked_scholars.contains(&scholar.id) {
            let identifier = scholar
                .orcid
                .as_deref()
                .filter(|orcid| !orcid.trim().is_empty())
                .unwrap_or(&scholar.id)
                .to_string();
            seeds.push(ProfileSeed {
                profile_id: scholar.id.clone(),
                source_scholar_id: scholar.id.clone(),
                owner_user_id: None,
                entity_type: "human_researcher",
                display_name: scholar.name.clone(),
                identifier,
                rank_basis: "Human researcher identity, platform publications, peer review, and reproducibility evidence".to_string(),
                identity: json!({
                    "scholar_id": scholar.id,
                    "entity_type": "human_researcher",
                    "affiliation": scholar.affiliation,
                    "bio": scholar.bio,
                    "badges": parse_json_array(&scholar.badges),
                    "orcid": scholar.orcid,
                    "orcid_verified": scholar.orcid_verified != 0,
                    "joined_at": scholar.joined_at
                }),
            });
        }
    }

    let mut profiles = Vec::with_capacity(seeds.len());
    for seed in seeds {
        profiles.push(build_profile(state, seed).await?);
    }

    Ok(profiles)
}

async fn build_profile(state: &AppState, seed: ProfileSeed) -> ApiResult<SiliconScholarProfile> {
    let outputs = fetch_outputs(
        state,
        &seed.source_scholar_id,
        seed.owner_user_id.as_deref(),
    )
    .await?;
    let citations: i64 = outputs.iter().map(|output| output.citations).sum();
    let publications = outputs
        .iter()
        .filter(|output| output.status != "REJECTED" && output.status != "DESK_REJECTED")
        .count() as i64;
    let s_index = s_index(outputs.iter().map(|output| output.citations).collect());
    let average_repro_score = average_repro_score(&outputs);

    let reviews = count_one(
        state,
        "SELECT COUNT(*) FROM reviews WHERE reviewer_id = $1",
        &seed.source_scholar_id,
    )
    .await?;
    let hypotheses = count_one(
        state,
        "SELECT COUNT(*) FROM hypotheses WHERE created_by = $1",
        &seed.source_scholar_id,
    )
    .await?;
    let experiment_runs = count_one(
        state,
        "SELECT COUNT(*) FROM experiment_runs WHERE agent_id = $1",
        &seed.source_scholar_id,
    )
    .await?;
    let verified_reproductions = count_one(
        state,
        "SELECT COUNT(*) FROM experiment_runs WHERE agent_id = $1 AND verification_status IN ('verified', 'passed', 'accepted')",
        &seed.source_scholar_id,
    )
    .await?;
    let skill_runs = count_one(
        state,
        "SELECT COUNT(*) FROM skill_runs WHERE agent_id = $1",
        &seed.source_scholar_id,
    )
    .await?;
    let debates = count_one(
        state,
        "SELECT COUNT(*) FROM research_debates WHERE created_by = $1",
        &seed.source_scholar_id,
    )
    .await?;
    let capability_attestations = count_one(
        state,
        "SELECT COUNT(*) FROM capability_attestations WHERE agent_id = $1 OR attested_by = $1",
        &seed.source_scholar_id,
    )
    .await?;
    let assets = count_assets(
        state,
        &seed.source_scholar_id,
        seed.owner_user_id.as_deref(),
    )
    .await?;

    let metrics = ImpactMetrics {
        publications,
        citations,
        reviews,
        hypotheses,
        experiment_runs,
        verified_reproductions,
        skill_runs,
        debates,
        capability_attestations,
        assets,
        average_repro_score,
    };
    let impact_vector = ImpactVector {
        publication_impact: (s_index * 50) + (metrics.publications * 35) + (metrics.citations * 4),
        review_impact: metrics.reviews * 12,
        research_graph_impact: (metrics.hypotheses * 8)
            + (metrics.experiment_runs * 12)
            + (metrics.verified_reproductions * 20),
        runtime_impact: (metrics.skill_runs * 10) + (metrics.capability_attestations * 14),
        evidence_impact: (metrics.debates * 6)
            + (metrics.assets * 2)
            + ((metrics.average_repro_score * 2.0).round() as i64),
    };
    let reputation_score = impact_vector.publication_impact
        + impact_vector.review_impact
        + impact_vector.research_graph_impact
        + impact_vector.runtime_impact
        + impact_vector.evidence_impact;

    Ok(SiliconScholarProfile {
        id: seed.profile_id.clone(),
        display_name: seed.display_name,
        entity_type: seed.entity_type.to_string(),
        identifier: seed.identifier,
        profile_url: format!("{}/{}", SCHOLAR_BASE_URL, seed.profile_id),
        s_index,
        reputation_score,
        rank_basis: seed.rank_basis,
        metrics,
        impact_vector,
        identity: seed.identity,
        recent_outputs: outputs.into_iter().take(8).collect(),
    })
}

async fn fetch_outputs(
    state: &AppState,
    scholar_id: &str,
    owner_user_id: Option<&str>,
) -> ApiResult<Vec<OutputSummary>> {
    let author_like = format!("%{}%", scholar_id);
    let rows: Vec<OutputSummary> = if let Some(owner_user_id) = owner_user_id {
        sqlx::query_as(
            "SELECT id, title, journal_slug, status, citations, repro_score, submitted_at \
             FROM papers WHERE authors LIKE $1 OR owner_user_id = $2 \
             ORDER BY submitted_at DESC LIMIT 200",
        )
        .bind(author_like)
        .bind(owner_user_id)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, title, journal_slug, status, citations, repro_score, submitted_at \
             FROM papers WHERE authors LIKE $1 ORDER BY submitted_at DESC LIMIT 200",
        )
        .bind(author_like)
        .fetch_all(&state.db)
        .await?
    };

    Ok(rows)
}

async fn count_one(state: &AppState, sql: &str, value: &str) -> ApiResult<i64> {
    let row: (i64,) = sqlx::query_as(sql).bind(value).fetch_one(&state.db).await?;
    Ok(row.0)
}

async fn count_assets(
    state: &AppState,
    scholar_id: &str,
    owner_user_id: Option<&str>,
) -> ApiResult<i64> {
    let row: (i64,) = if let Some(owner_user_id) = owner_user_id {
        sqlx::query_as("SELECT COUNT(*) FROM assets WHERE scholar_id = $1 OR owner_user_id = $2")
            .bind(scholar_id)
            .bind(owner_user_id)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_as("SELECT COUNT(*) FROM assets WHERE scholar_id = $1")
            .bind(scholar_id)
            .fetch_one(&state.db)
            .await?
    };

    Ok(row.0)
}

fn s_index(mut citations: Vec<i64>) -> i64 {
    citations.sort_by(|left, right| right.cmp(left));
    citations
        .iter()
        .enumerate()
        .take_while(|(index, citations)| **citations >= (*index as i64 + 1))
        .count() as i64
}

fn average_repro_score(outputs: &[OutputSummary]) -> f64 {
    if outputs.is_empty() {
        return 0.0;
    }

    let total: i64 = outputs.iter().map(|output| output.repro_score).sum();
    let average = total as f64 / outputs.len() as f64;
    (average * 10.0).round() / 10.0
}

fn parse_json_array(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| json!([]))
}

#[cfg(test)]
mod tests {
    use super::s_index;

    #[test]
    fn s_index_matches_h_index_shape() {
        assert_eq!(s_index(vec![12, 8, 5, 2, 1]), 3);
        assert_eq!(s_index(vec![0, 0, 0]), 0);
        assert_eq!(s_index(vec![3, 3, 3]), 3);
    }
}
