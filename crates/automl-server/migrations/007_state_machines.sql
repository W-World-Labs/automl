-- Formalized state machines (see docs/STATE-MACHINES.md — the authority).
-- Every platform-managed status column now carries a CHECK; transition rules
-- live in the domain layer with unit tests mirroring the documented tables.

-- papers.code_status: unknown | repo_pending | repo_ready | ready
ALTER TABLE papers ADD CONSTRAINT papers_code_status_valid
    CHECK (code_status IN ('unknown', 'repo_pending', 'repo_ready', 'ready'));

-- reproduce_runs: add the timeout terminal state `stale`
-- (registered runs whose evidence never arrives get swept by w-task).
ALTER TABLE reproduce_runs DROP CONSTRAINT reproduce_runs_status_check;
ALTER TABLE reproduce_runs ADD CONSTRAINT reproduce_runs_status_check
    CHECK (status IN ('registered', 'evidence_submitted', 'accepted', 'rejected', 'stale'));
