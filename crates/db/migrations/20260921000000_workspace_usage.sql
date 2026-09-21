-- Usage describes the human operation contract, not filesystem ownership,
-- visibility, a Workflow template's source, or an expiring reservation.
ALTER TABLE workspaces ADD COLUMN usage TEXT NOT NULL DEFAULT 'interactive'
    CHECK (usage IN ('interactive', 'execution_only'));
ALTER TABLE workspaces ADD COLUMN execution_owner TEXT
    CHECK (execution_owner IS NULL OR json_valid(execution_owner));
CREATE INDEX idx_workspaces_usage ON workspaces(usage, archived, updated_at);

-- Evidence-based backfill runs in the existing startup reconciliation path.
-- Do not guess from names or rewrite archive/Session/Audit/Git history here.
