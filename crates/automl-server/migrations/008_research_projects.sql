-- Research Hub P0: project skeleton (docs/RESEARCH-HUB-DESIGN.md §12.1/§12.7).
-- research_projects is the lab work unit; project_members carries the project
-- role that intersects global scopes; audit_events is the append-only trail.
-- Fuel (project_papers) and research_run are P1+/P3 — deliberately absent here.

CREATE TABLE research_projects (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug                 TEXT NOT NULL,
    title                TEXT NOT NULL,
    summary              TEXT NOT NULL DEFAULT '',
    visibility           TEXT NOT NULL DEFAULT 'private'
        CHECK (visibility IN ('private', 'lab', 'public_meta')),
    status               TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'active', 'archived', 'deleted')),
    -- Reserved lab/org pointer; no lab table yet (P0 stub, nullable).
    lab_id               UUID NULL,
    -- Optional W-DaaS workspace binding (project_binding). External pointer —
    -- a TEXT id, never a FK into another product's schema.
    w_daas_workspace_id  TEXT NULL,
    created_by           UUID NOT NULL REFERENCES scholars (id),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT research_projects_title_nonempty CHECK (char_length(title) > 0),
    CONSTRAINT research_projects_slug_format
        CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,63}$')
);

-- Slug is unique per owner (design §12.1: owner 下唯一). Soft-deleted rows free
-- the slug so a successor project can reuse it.
CREATE UNIQUE INDEX research_projects_owner_slug_idx
    ON research_projects (created_by, slug) WHERE status <> 'deleted';
CREATE INDEX research_projects_list_idx ON research_projects (created_at DESC, id DESC);
CREATE INDEX research_projects_created_by_idx ON research_projects (created_by);

CREATE TABLE project_members (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id     UUID NOT NULL REFERENCES research_projects (id) ON DELETE CASCADE,
    -- §12.5: agents attach as rows too, but a P0 member-add accepts scholars
    -- only; an API-key caller inherits its owner scholar's membership.
    principal_type TEXT NOT NULL CHECK (principal_type IN ('scholar', 'agent')),
    scholar_id     UUID NULL REFERENCES scholars (id) ON DELETE CASCADE,
    agent_id       UUID NULL REFERENCES agents (id) ON DELETE CASCADE,
    role           TEXT NOT NULL CHECK (role IN ('owner', 'member', 'viewer')),
    added_by       UUID NULL REFERENCES scholars (id),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_members_principal_match CHECK (
        (principal_type = 'scholar' AND scholar_id IS NOT NULL AND agent_id IS NULL)
        OR (principal_type = 'agent' AND agent_id IS NOT NULL AND scholar_id IS NULL)
    ),
    -- §12.1 agent carve-out at the data layer: agents may never hold Owner, so
    -- no agent principal can reach governance ops (ACL / archive / delete).
    CONSTRAINT project_members_agent_not_owner
        CHECK (principal_type = 'scholar' OR role <> 'owner')
);

CREATE UNIQUE INDEX project_members_scholar_uniq
    ON project_members (project_id, scholar_id) WHERE principal_type = 'scholar';
CREATE UNIQUE INDEX project_members_agent_uniq
    ON project_members (project_id, agent_id) WHERE principal_type = 'agent';
CREATE INDEX project_members_list_idx
    ON project_members (project_id, created_at DESC, id DESC);
CREATE INDEX project_members_scholar_idx
    ON project_members (scholar_id) WHERE principal_type = 'scholar';

-- Append-only audit trail: no UPDATE/DELETE ever. The trigger below is the
-- DB-level guarantee; domain code only ever INSERTs.
CREATE TABLE audit_events (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES research_projects (id) ON DELETE CASCADE,
    -- Who acted: the owning scholar for API keys as well (accountability rule).
    actor_id   UUID NULL REFERENCES scholars (id),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('scholar', 'api_key', 'system')),
    action     TEXT NOT NULL,
    detail     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_events_project_idx
    ON audit_events (project_id, created_at DESC, id DESC);

CREATE FUNCTION audit_events_reject_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER audit_events_append_only
    BEFORE UPDATE OR DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_reject_mutation();
