-- Iteration 8: one-time authorization codes for the CLI loopback login flow.
-- Browser (W-ID session) approves → code → CLI exchanges code+PKCE verifier
-- for a freshly minted API key. Codes are stored hashed, single-use, short TTL.

CREATE TABLE cli_auth_codes (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code_hash     CHAR(64) NOT NULL UNIQUE,
    scholar_id    UUID NOT NULL REFERENCES scholars (id) ON DELETE CASCADE,
    -- hex(sha256(pkce_verifier))
    challenge     CHAR(64) NOT NULL,
    redirect_uri  TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL,
    used_at       TIMESTAMPTZ NULL,
    CONSTRAINT cli_auth_codes_hash_hex CHECK (code_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT cli_auth_codes_challenge_hex CHECK (challenge ~ '^[0-9a-f]{64}$')
);

CREATE INDEX cli_auth_codes_expiry_idx ON cli_auth_codes (expires_at);
