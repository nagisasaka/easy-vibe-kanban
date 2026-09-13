-- Preserve all existing run identities and their referencing node/attempt rows.
-- As in the workspace FK migration, SQLx's outer transaction must be ended
-- before changing SQLite foreign-key enforcement for a table rebuild.
COMMIT;
PRAGMA foreign_keys = OFF;
BEGIN TRANSACTION;

CREATE TABLE workflow_runs_repository_scope (
    id BLOB PRIMARY KEY,
    workflow_id BLOB NOT NULL REFERENCES workflows(id) ON DELETE RESTRICT,
    issue_id BLOB REFERENCES local_issues(id) ON DELETE CASCADE,
    repository_id BLOB REFERENCES repos(id) ON DELETE RESTRICT,
    workspace_id BLOB REFERENCES workspaces(id) ON DELETE SET NULL,
    trigger_source TEXT NOT NULL DEFAULT 'manual',
    input_text TEXT NOT NULL,
    output_text TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
        'pending', 'running', 'awaiting_human', 'awaiting_arena', 'cancelling',
        'succeeded', 'failed', 'canceled'
    )),
    started_at TEXT,
    finished_at TEXT,
    error_text TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    attempt_id BLOB REFERENCES workflow_attempts(id) ON DELETE SET NULL,
    orchestration_run_id BLOB REFERENCES orchestration_runs(id) ON DELETE SET NULL,
    graph_snapshot TEXT,
    CHECK ((issue_id IS NOT NULL AND repository_id IS NULL) OR
           (issue_id IS NULL AND repository_id IS NOT NULL AND attempt_id IS NULL))
);

INSERT INTO workflow_runs_repository_scope
    (id, workflow_id, issue_id, workspace_id, trigger_source, input_text,
     output_text, status, started_at, finished_at, error_text, created_at,
     updated_at, attempt_id, orchestration_run_id, graph_snapshot)
SELECT id, workflow_id, issue_id, workspace_id, trigger_source, input_text,
       output_text, status, started_at, finished_at, error_text, created_at,
       updated_at, attempt_id, orchestration_run_id, graph_snapshot
FROM workflow_runs;

DROP TABLE workflow_runs;
ALTER TABLE workflow_runs_repository_scope RENAME TO workflow_runs;
CREATE INDEX idx_workflow_runs_issue_id ON workflow_runs(issue_id);
CREATE INDEX idx_workflow_runs_repository_id ON workflow_runs(repository_id);
CREATE INDEX idx_workflow_runs_status ON workflow_runs(status);
CREATE INDEX idx_workflow_runs_attempt_id ON workflow_runs(attempt_id);
CREATE UNIQUE INDEX idx_workflow_runs_orchestration_run_id
    ON workflow_runs(orchestration_run_id) WHERE orchestration_run_id IS NOT NULL;
PRAGMA foreign_key_check;
COMMIT;
PRAGMA foreign_keys = ON;
BEGIN TRANSACTION;
