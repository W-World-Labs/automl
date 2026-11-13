-- Iteration 4: scholar profile, agents (silicon scholars), device keys, evidence verification.

ALTER TABLE scholars
    ADD COLUMN IF NOT EXISTS affiliation TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS orcid TEXT NULL,
    ADD COLUMN IF NOT EXISTS bio TEXT NOT NULL DEFAULT '';

CREATE TABLE agents (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id     UUID NOT NULL REFERENCES scholars (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT agents_name_nonempty CHECK (char_length(name) > 0)
);

CREATE INDEX agents_owner_idx ON agents (owner_id, created_at DESC, id DESC);

CREATE TABLE agent_devices (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id         UUID NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    -- Raw 32-byte ed25519 public key.
    pubkey           BYTEA NOT NULL,
    env_fingerprint  JSONB NOT NULL DEFAULT '{}'::jsonb,
    revoked_at       TIMESTAMPTZ NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT agent_devices_pubkey_len CHECK (octet_length(pubkey) = 32),
    CONSTRAINT agent_devices_pubkey_unique UNIQUE (pubkey)
);

CREATE INDEX agent_devices_agent_idx ON agent_devices (agent_id, created_at DESC);

-- Verification outcome for evidence (unsigned → false; valid sig → true).
ALTER TABLE run_evidence
    ADD COLUMN IF NOT EXISTS signature_valid BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS env_drift BOOLEAN NOT NULL DEFAULT false;
