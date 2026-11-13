// ============================================================
// Database — PostgreSQL via sqlx
// ============================================================

use sqlx::postgres::{PgPool, PgPoolOptions};

pub type Pool = PgPool;

pub async fn init_pool(database_url: &str) -> anyhow::Result<Pool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &Pool) -> anyhow::Result<()> {
    sqlx::raw_sql(SCHEMA).execute(pool).await?;
    sqlx::raw_sql(w_id_sdk::tenancy::RECOMMENDED_SCHEMA)
        .execute(pool)
        .await?;
    sqlx::raw_sql(COMPAT_SCHEMA).execute(pool).await?;
    Ok(())
}

const SCHEMA: &str = r#"
-- Auto.ml Schema — PostgreSQL 18, clean DB, no legacy data import.

CREATE TABLE IF NOT EXISTS scholars (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    type TEXT NOT NULL DEFAULT 'human',
    affiliation TEXT NOT NULL DEFAULT '',
    bio TEXT NOT NULL DEFAULT '',
    badges TEXT NOT NULL DEFAULT '[]',
    orcid TEXT,
    orcid_verified BIGINT NOT NULL DEFAULT 0,
    openclaw_handle TEXT,
    lineage TEXT,
    joined_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS papers (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    abstract TEXT NOT NULL DEFAULT '',
    domain TEXT NOT NULL DEFAULT 'Uncategorized',
    journal_slug TEXT NOT NULL,
    authors TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'SUBMITTED',
    compute_tier TEXT NOT NULL DEFAULT 'nano',
    submitted_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    citations BIGINT NOT NULL DEFAULT 0,
    repro_score BIGINT NOT NULL DEFAULT 0,
    pdf_key TEXT,
    pdf_url TEXT,
    github_repo TEXT,
    owner_user_id TEXT,
    workspace_id TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS reviews (
    id TEXT PRIMARY KEY,
    paper_id TEXT NOT NULL REFERENCES papers(id),
    reviewer_id TEXT NOT NULL,
    verdict TEXT NOT NULL,
    score BIGINT NOT NULL DEFAULT 5,
    confidence BIGINT NOT NULL DEFAULT 3,
    summary TEXT NOT NULL DEFAULT '',
    strengths TEXT NOT NULL DEFAULT '[]',
    weaknesses TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS activity_events (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    message TEXT NOT NULL,
    actor_id TEXT,
    paper_id TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY,
    scholar_id TEXT NOT NULL,
    owner_user_id TEXT,
    workspace_id TEXT,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    size_bytes BIGINT NOT NULL,
    sha256 TEXT NOT NULL,
    r2_key TEXT NOT NULL,
    asset_type TEXT NOT NULL DEFAULT 'reference',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS api_keys (
    key_hash TEXT PRIMARY KEY,
    scholar_id TEXT NOT NULL,
    key_name TEXT NOT NULL DEFAULT 'Default',
    trust_level TEXT NOT NULL DEFAULT 'unverified',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    expires_at TEXT
);

CREATE TABLE IF NOT EXISTS agent_devices (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    hostname TEXT,
    gpu_info TEXT,
    enrollment_token TEXT,
    device_fingerprint TEXT,
    enrolled_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    last_seen_at TEXT
);

CREATE TABLE IF NOT EXISTS agent_enrollments (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    expires_at TEXT NOT NULL,
    used_at TEXT
);

CREATE TABLE IF NOT EXISTS sso_users (
    user_id TEXT PRIMARY KEY,
    scholar_id TEXT NOT NULL UNIQUE,
    login TEXT NOT NULL DEFAULT '',
    email TEXT,
    name TEXT,
    is_admin BIGINT NOT NULL DEFAULT 0,
    last_login_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS forum_posts (
    id TEXT PRIMARY KEY,
    author_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    replies TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS feedback (
    id TEXT PRIMARY KEY,
    author_id TEXT,
    type TEXT NOT NULL DEFAULT 'suggestion',
    message TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    reply TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS skills (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    author_id TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0',
    downloads BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS research_agendas (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT '',
    owner_user_id TEXT NOT NULL,
    workspace_id TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS hypotheses (
    id TEXT PRIMARY KEY,
    agenda_id TEXT NOT NULL REFERENCES research_agendas(id),
    parent_hypothesis_id TEXT REFERENCES hypotheses(id),
    claim TEXT NOT NULL,
    novelty_score DOUBLE PRECISION,
    status TEXT NOT NULL DEFAULT 'proposed',
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS experiment_plans (
    id TEXT PRIMARY KEY,
    hypothesis_id TEXT NOT NULL REFERENCES hypotheses(id),
    manifest_ref TEXT NOT NULL DEFAULT '',
    resource_tier TEXT NOT NULL DEFAULT 'nano',
    safety_scope TEXT NOT NULL DEFAULT 'software-only',
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS experiment_runs (
    id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL REFERENCES experiment_plans(id),
    agent_id TEXT NOT NULL,
    artifact_ref TEXT NOT NULL DEFAULT '',
    metrics_json TEXT NOT NULL DEFAULT '{}',
    verification_status TEXT NOT NULL DEFAULT 'unverified',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS skill_runs (
    id TEXT PRIMARY KEY,
    hypothesis_id TEXT NOT NULL REFERENCES hypotheses(id),
    skill_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    runtime TEXT NOT NULL DEFAULT '',
    inputs_ref TEXT NOT NULL DEFAULT '',
    outputs_ref TEXT NOT NULL DEFAULT '',
    logs_ref TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'submitted',
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS research_debates (
    id TEXT PRIMARY KEY,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    participants_json TEXT NOT NULL DEFAULT '[]',
    transcript_ref TEXT NOT NULL DEFAULT '',
    verdict TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS capability_attestations (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    radar_project_id TEXT NOT NULL DEFAULT '',
    runtime TEXT NOT NULL DEFAULT '',
    model_or_engine TEXT NOT NULL DEFAULT '',
    tooling_surface TEXT NOT NULL DEFAULT '',
    supported_skills_json TEXT NOT NULL DEFAULT '[]',
    benchmark_score_json TEXT NOT NULL DEFAULT '{}',
    evidence_policy TEXT NOT NULL DEFAULT '',
    attested_by TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS webhooks (
    id TEXT PRIMARY KEY,
    scholar_id TEXT NOT NULL,
    url TEXT NOT NULL,
    events TEXT NOT NULL DEFAULT '[]',
    secret TEXT,
    active BIGINT NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text)
);

CREATE TABLE IF NOT EXISTS reproduction_jobs (
    id TEXT PRIMARY KEY,
    paper_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    backend TEXT NOT NULL DEFAULT 'local',
    runtime TEXT NOT NULL DEFAULT '',
    command TEXT NOT NULL DEFAULT '',
    exit_code BIGINT,
    repro_score BIGINT NOT NULL DEFAULT 0,
    metrics_json TEXT NOT NULL DEFAULT '{}',
    output_hashes TEXT NOT NULL DEFAULT '{}',
    log_excerpt TEXT NOT NULL DEFAULT '',
    sandbox_id TEXT,
    requested_by TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::text),
    started_at TEXT,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_papers_journal ON papers(journal_slug);
CREATE INDEX IF NOT EXISTS idx_papers_status ON papers(status);
CREATE INDEX IF NOT EXISTS idx_papers_submitted ON papers(submitted_at DESC);
CREATE INDEX IF NOT EXISTS idx_reviews_paper ON reviews(paper_id);
CREATE INDEX IF NOT EXISTS idx_activity_created ON activity_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_scholars_type ON scholars(type);
CREATE INDEX IF NOT EXISTS idx_assets_scholar ON assets(scholar_id);
CREATE INDEX IF NOT EXISTS idx_assets_sha256 ON assets(sha256);
CREATE INDEX IF NOT EXISTS idx_api_keys_scholar ON api_keys(scholar_id);
CREATE INDEX IF NOT EXISTS idx_papers_owner ON papers(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_papers_workspace ON papers(workspace_id);
CREATE INDEX IF NOT EXISTS idx_agent_enrollments_agent ON agent_enrollments(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_devices_agent ON agent_devices(agent_id);
CREATE INDEX IF NOT EXISTS idx_research_agendas_owner ON research_agendas(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_research_agendas_workspace ON research_agendas(workspace_id);
CREATE INDEX IF NOT EXISTS idx_hypotheses_agenda ON hypotheses(agenda_id);
CREATE INDEX IF NOT EXISTS idx_skill_runs_hypothesis ON skill_runs(hypothesis_id);
CREATE INDEX IF NOT EXISTS idx_skill_runs_agent ON skill_runs(agent_id);
CREATE INDEX IF NOT EXISTS idx_research_debates_target ON research_debates(target_type, target_id);
CREATE INDEX IF NOT EXISTS idx_capability_attestations_agent ON capability_attestations(agent_id);
CREATE INDEX IF NOT EXISTS idx_reproduction_jobs_paper ON reproduction_jobs(paper_id);
CREATE INDEX IF NOT EXISTS idx_reproduction_jobs_status ON reproduction_jobs(status);
"#;

const COMPAT_SCHEMA: &str = r#"
ALTER TABLE papers ADD COLUMN IF NOT EXISTS owner_user_id TEXT;
ALTER TABLE papers ADD COLUMN IF NOT EXISTS workspace_id TEXT;
ALTER TABLE assets ADD COLUMN IF NOT EXISTS owner_user_id TEXT;
ALTER TABLE assets ADD COLUMN IF NOT EXISTS workspace_id TEXT;
ALTER TABLE agent_devices ADD COLUMN IF NOT EXISTS device_fingerprint TEXT;
ALTER TABLE agent_devices ADD COLUMN IF NOT EXISTS last_seen_at TEXT;
"#;
