-- Research Hub P2: W-DaaS workspace bindings (project_binding) and project
-- asset pointers (project_asset), docs/RESEARCH-HUB-DESIGN.md §12.3/§12.7.
CREATE TABLE project_bindings (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id   UUID NOT NULL REFERENCES research_projects (id) ON DELETE CASCADE,
    provider     TEXT NOT NULL DEFAULT 'w_daas' CHECK (provider IN ('w_daas')),
    workspace_id TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'unbound')),
    -- Whether the provider confirmed the workspace at bind time. The stub
    -- provider can never verify, so false is the honest default.
    verified     BOOLEAN NOT NULL DEFAULT false,
    created_by   UUID NOT NULL REFERENCES scholars (id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    unbound_at   TIMESTAMPTZ NULL,
    CONSTRAINT project_bindings_workspace_nonempty
        CHECK (char_length(btrim(workspace_id)) > 0)
);
-- One live binding per project (design §12.7 P2: project ↔ workspace is 1:1).
CREATE UNIQUE INDEX project_bindings_active_uniq
    ON project_bindings (project_id) WHERE status = 'active';
CREATE INDEX project_bindings_project_idx
    ON project_bindings (project_id, created_at DESC, id DESC);

-- Fold the P0 unvalidated workspace pointer into the binding log so the
-- project column and the binding table tell one story.
INSERT INTO project_bindings (project_id, workspace_id, created_by, status, verified)
SELECT id, w_daas_workspace_id, created_by, 'active', false
FROM research_projects
WHERE w_daas_workspace_id IS NOT NULL AND btrim(w_daas_workspace_id) <> '';

CREATE TABLE project_assets (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id       UUID NOT NULL REFERENCES research_projects (id) ON DELETE CASCADE,
    -- design §12.3 kind set; content kinds carry payloads in metadata, the
    -- rest are pointers to a DaaS artifact or an AutoML/w-storage object.
    kind             TEXT NOT NULL CHECK (kind IN (
        'note', 'search_snapshot', 'kg_slice', 'research_draft',
        'experiment_spec', 'sim_config', 'result_bundle', 'figure',
        'notebook_ptr'
    )),
    title            TEXT NOT NULL DEFAULT '',
    uri              TEXT NULL,
    daas_artifact_id TEXT NULL,
    metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_by       UUID NOT NULL REFERENCES scholars (id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at       TIMESTAMPTZ NULL,
    CONSTRAINT project_assets_pointer_required CHECK (
        (uri IS NOT NULL AND btrim(uri) <> '')
        OR (daas_artifact_id IS NOT NULL AND btrim(daas_artifact_id) <> '')
        OR kind IN ('note', 'search_snapshot')
    )
);
CREATE INDEX project_assets_list_idx
    ON project_assets (project_id, created_at DESC, id DESC)
    WHERE deleted_at IS NULL;
