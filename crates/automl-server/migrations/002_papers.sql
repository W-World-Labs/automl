-- Iteration 3: papers core — code pointers only; reproduce evidence; assets.

CREATE TABLE papers (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title           TEXT NOT NULL,
    abstract        TEXT NOT NULL DEFAULT '',
    authors         JSONB NOT NULL DEFAULT '[]'::jsonb,
    repo_url        TEXT NOT NULL,
    commit_sha      CHAR(40) NOT NULL,
    reproduce_path  TEXT NOT NULL DEFAULT 'reproduce.yaml',
    code_status     TEXT NOT NULL DEFAULT 'unknown',
    review_status   TEXT NOT NULL DEFAULT 'draft'
        CHECK (review_status IN ('draft', 'submitted', 'under_review', 'accepted', 'rejected')),
    submitted_by    UUID NOT NULL REFERENCES scholars (id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT papers_title_nonempty CHECK (char_length(title) > 0),
    CONSTRAINT papers_commit_sha_hex CHECK (commit_sha ~ '^[0-9a-f]{40}$')
);

CREATE INDEX papers_list_idx ON papers (created_at DESC, id DESC);
CREATE INDEX papers_submitted_by_idx ON papers (submitted_by);

CREATE TABLE reviews (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    paper_id     UUID NOT NULL REFERENCES papers (id) ON DELETE CASCADE,
    reviewer_id  UUID NOT NULL REFERENCES scholars (id),
    verdict      TEXT NOT NULL CHECK (verdict IN ('approve', 'reject', 'revise')),
    body         TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX reviews_paper_idx ON reviews (paper_id, created_at DESC, id DESC);

CREATE TABLE reproduce_runs (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    paper_id              UUID NOT NULL REFERENCES papers (id) ON DELETE CASCADE,
    spec_hash             CHAR(64) NOT NULL,
    runner_principal      TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'registered'
        CHECK (status IN ('registered', 'evidence_submitted', 'accepted', 'rejected')),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    evidence_submitted_at TIMESTAMPTZ NULL,
    CONSTRAINT reproduce_runs_spec_hash_hex CHECK (spec_hash ~ '^[0-9a-f]{64}$')
);

CREATE INDEX reproduce_runs_paper_idx ON reproduce_runs (paper_id, created_at DESC);
CREATE INDEX reproduce_runs_spec_idx ON reproduce_runs (paper_id, spec_hash, status);

CREATE TABLE run_evidence (
    run_id           UUID PRIMARY KEY REFERENCES reproduce_runs (id) ON DELETE CASCADE,
    exit_code        INT NOT NULL,
    logs_object_key  TEXT NULL,
    artifact_hashes  JSONB NOT NULL DEFAULT '{}'::jsonb,
    metrics          JSONB NOT NULL DEFAULT '{}'::jsonb,
    env_fingerprint  JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Stored but not verified until iteration 4 (device keys).
    signature        BYTEA NULL,
    signer_key_id    UUID NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE assets (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    paper_id      UUID NOT NULL REFERENCES papers (id) ON DELETE CASCADE,
    object_key    TEXT NOT NULL UNIQUE,
    sha256        CHAR(64) NOT NULL,
    size_bytes    BIGINT NOT NULL CHECK (size_bytes >= 0),
    content_type  TEXT NOT NULL DEFAULT 'application/octet-stream',
    status        TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'committed')),
    created_by    UUID NOT NULL REFERENCES scholars (id),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT assets_sha256_hex CHECK (sha256 ~ '^[0-9a-f]{64}$')
);

CREATE INDEX assets_paper_idx ON assets (paper_id, created_at DESC);
