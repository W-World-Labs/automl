-- Iteration 5: client compute profiles, templates + manifest, dedicated Gitea integration.

-- Client-reported compute environment profiles. Provenance only — the platform
-- never schedules work onto these environments.
CREATE TABLE compute_profiles (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      UUID NOT NULL REFERENCES scholars (id) ON DELETE CASCADE,
    agent_id      UUID NULL REFERENCES agents (id) ON DELETE CASCADE,
    profile       JSONB NOT NULL,
    profile_hash  CHAR(64) NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT compute_profiles_hash_hex CHECK (profile_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT compute_profiles_owner_hash_uniq UNIQUE (owner_id, profile_hash)
);

CREATE INDEX compute_profiles_list_idx ON compute_profiles (created_at DESC, id DESC);
CREATE INDEX compute_profiles_agent_idx ON compute_profiles (agent_id);

-- Versioned templates (paper skeletons, reproduce.yaml templates) with a
-- distributable skills manifest.
CREATE TABLE templates (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('paper', 'reproduce', 'skill')),
    body        TEXT NOT NULL DEFAULT '',
    manifest    JSONB NOT NULL DEFAULT '{}'::jsonb,
    version     INT NOT NULL DEFAULT 1,
    created_by  UUID NOT NULL REFERENCES scholars (id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT templates_name_nonempty CHECK (char_length(name) > 0)
);

CREATE INDEX templates_list_idx ON templates (created_at DESC, id DESC);

-- Platform-hosted code: papers may now be created without an external repo
-- pointer; the platform provisions a repo on the dedicated Gitea instance.
-- commit_sha '' means "no pinned commit yet" (reproduce runs are refused).
ALTER TABLE papers ALTER COLUMN commit_sha TYPE TEXT;
ALTER TABLE papers DROP CONSTRAINT papers_commit_sha_hex;
ALTER TABLE papers ADD CONSTRAINT papers_commit_sha_hex
    CHECK (commit_sha ~ '^([0-9a-f]{40})?$');

-- Latest commit observed via Gitea push webhook. Informational sync only —
-- commit_sha stays the author-pinned commit used for reproduction.
ALTER TABLE papers ADD COLUMN latest_commit TEXT NULL
    CHECK (latest_commit ~ '^[0-9a-f]{40}$');
