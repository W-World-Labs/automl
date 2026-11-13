-- Iteration 2: minimal scholar row + API key lifecycle.
-- Scholar profile fields land in iteration 4; here we only need a stable
-- owner for keys and session→principal mapping (w_id + display_name).

CREATE TABLE scholars (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    w_id         TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE api_keys (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id     UUID NOT NULL REFERENCES scholars (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- Raw SHA-256 of the full plaintext key (32 bytes). Never store plaintext.
    key_sha256   BYTEA NOT NULL UNIQUE,
    scopes       TEXT[] NOT NULL DEFAULT '{}',
    expires_at   TIMESTAMPTZ NULL,
    last_used_at TIMESTAMPTZ NULL,
    revoked_at   TIMESTAMPTZ NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT api_keys_name_nonempty CHECK (char_length(name) > 0),
    CONSTRAINT api_keys_sha256_len CHECK (octet_length(key_sha256) = 32)
);

CREATE INDEX api_keys_owner_id_idx ON api_keys (owner_id);
CREATE INDEX api_keys_owner_active_idx
    ON api_keys (owner_id, created_at DESC, id DESC)
    WHERE revoked_at IS NULL;
