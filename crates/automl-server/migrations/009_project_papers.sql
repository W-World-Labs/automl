-- Research Hub P1 fuel shelf: external paper pointers and cached metadata.
CREATE TABLE project_papers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES research_projects(id) ON DELETE CASCADE,
    fuel_role TEXT NOT NULL DEFAULT 'seed'
        CHECK (fuel_role IN ('seed', 'related', 'baseline', 'cited_by', 'reading')),
    doi TEXT NULL,
    arxiv_id TEXT NULL,
    openalex_id TEXT NULL,
    w_papers_work_id TEXT NULL,
    daas_document_id TEXT NULL,
    paper_id UUID NULL REFERENCES papers(id) ON DELETE SET NULL,
    notes TEXT NOT NULL DEFAULT '',
    tags TEXT[] NOT NULL DEFAULT '{}',
    cached_title TEXT NULL,
    cached_year INTEGER NULL CHECK (cached_year IS NULL OR cached_year BETWEEN 1000 AND 3000),
    cached_authors JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(cached_authors) = 'array'),
    created_by UUID NOT NULL REFERENCES scholars(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ NULL,
    CONSTRAINT project_papers_identifier_present CHECK (
        num_nonnulls(doi, arxiv_id, openalex_id, w_papers_work_id, daas_document_id) >= 1
    )
);
CREATE UNIQUE INDEX project_papers_project_doi_idx
    ON project_papers(project_id, lower(btrim(doi)))
    WHERE doi IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX project_papers_list_idx
    ON project_papers(project_id, created_at DESC, id DESC)
    WHERE deleted_at IS NULL;
