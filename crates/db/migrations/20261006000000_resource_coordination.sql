-- One authority per LVK database. Ownership never expires on a timer.
ALTER TABLE orchestration_runs ADD COLUMN product_kind_new TEXT NOT NULL
    DEFAULT 'workflow' CHECK(product_kind_new IN ('workflow','arena','integration','resource_mediation'));
UPDATE orchestration_runs SET product_kind_new=product_kind;
DROP INDEX idx_orchestration_runs_source_definition;
ALTER TABLE orchestration_runs DROP COLUMN product_kind;
ALTER TABLE orchestration_runs RENAME COLUMN product_kind_new TO product_kind;
CREATE INDEX idx_orchestration_runs_source_definition ON orchestration_runs(product_kind,source_definition_id);

CREATE TABLE shared_resources (
    id BLOB PRIMARY KEY NOT NULL,
    resource_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    fence INTEGER NOT NULL DEFAULT 0,
    health TEXT NOT NULL DEFAULT 'ready' CHECK(health IN ('ready','recovery_required')),
    created_at TEXT NOT NULL DEFAULT (datetime('now','subsec'))
);
CREATE TABLE resource_operations (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id BLOB NOT NULL UNIQUE,
    workspace_id BLOB NOT NULL,
    session_id BLOB NOT NULL,
    spec TEXT NOT NULL CHECK(json_valid(spec)),
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','blocked','launching','running','succeeded','cancelled','recovery_required','recovered')),
    priority INTEGER NOT NULL DEFAULT 0,
    process_id BLOB,
    runtime_id BLOB,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','subsec')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now','subsec'))
);
CREATE INDEX resource_operations_queue ON resource_operations(status,priority,sequence);
CREATE TABLE resource_holders (
    resource_id BLOB PRIMARY KEY NOT NULL REFERENCES shared_resources(id) ON DELETE RESTRICT,
    operation_id BLOB NOT NULL REFERENCES resource_operations(id) ON DELETE RESTRICT,
    fence INTEGER NOT NULL
);
CREATE INDEX resource_holders_operation ON resource_holders(operation_id);
CREATE TABLE resource_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    resource_id BLOB REFERENCES shared_resources(id) ON DELETE RESTRICT,
    operation_id BLOB REFERENCES resource_operations(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now','subsec'))
);
CREATE TABLE resource_mediations (
    id BLOB PRIMARY KEY NOT NULL,
    trigger_operation_id BLOB NOT NULL REFERENCES resource_operations(id) ON DELETE RESTRICT,
    snapshot TEXT NOT NULL CHECK(json_valid(snapshot)),
    status TEXT NOT NULL CHECK(status IN ('pending','preparing','running','applied','stale','failed')),
    workspace_id BLOB,
    session_id BLOB,
    agent_run_id BLOB,
    result TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','subsec'))
);
CREATE INDEX resource_mediations_trigger ON resource_mediations(trigger_operation_id);

-- Preserve existing values and constraints while introducing a script reason.
DROP TRIGGER integration_script_admission;
ALTER TABLE execution_processes ADD COLUMN run_reason_new TEXT NOT NULL DEFAULT 'setupscript'
    CHECK(run_reason_new IN ('setupscript','cleanupscript','archivescript','devserver','integrationvalidation','resourcecommand'));
UPDATE execution_processes SET run_reason_new=run_reason;
DROP INDEX idx_execution_processes_run_reason;
DROP INDEX idx_execution_processes_session_status_run_reason;
DROP INDEX idx_execution_processes_session_run_reason_created;
ALTER TABLE execution_processes DROP COLUMN run_reason;
ALTER TABLE execution_processes RENAME COLUMN run_reason_new TO run_reason;
CREATE INDEX idx_execution_processes_run_reason ON execution_processes(run_reason);
CREATE INDEX idx_execution_processes_session_status_run_reason ON execution_processes(session_id,status,run_reason);
CREATE INDEX idx_execution_processes_session_run_reason_created ON execution_processes(session_id,run_reason,created_at DESC);
CREATE TRIGGER integration_script_admission BEFORE INSERT ON execution_processes
WHEN EXISTS(SELECT 1 FROM integration_reservations r JOIN sessions s ON r.resource_key=lower(hex(s.workspace_id))
 WHERE r.resource_kind='workspace' AND s.id=NEW.session_id
 AND NOT EXISTS(SELECT 1 FROM integration_runs i WHERE i.id=r.run_id AND i.workspace_id=s.workspace_id
 AND i.session_id=s.id AND i.status='validating' AND NEW.run_reason='integrationvalidation'))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;

-- A Card/worktree cannot disappear while a managed operation can still act.
CREATE TRIGGER resource_workspace_change BEFORE UPDATE OF archived,worktree_deleted ON workspaces
WHEN (NEW.archived<>OLD.archived OR NEW.worktree_deleted<>OLD.worktree_deleted)
 AND EXISTS(SELECT 1 FROM resource_operations o WHERE o.workspace_id=OLD.id
 AND o.status IN ('queued','blocked','launching','running','recovery_required'))
BEGIN SELECT RAISE(ABORT,'Workspace has unresolved shared resource operations'); END;

-- Historical identities survive normal workspace deletion. Active work cannot
-- be removed to make a resource look idle; terminal audit rows are not cascaded.
CREATE TRIGGER resource_workspace_delete BEFORE DELETE ON workspaces
WHEN EXISTS(SELECT 1 FROM resource_operations o WHERE o.workspace_id=OLD.id
 AND o.status IN ('queued','blocked','launching','running','recovery_required'))
 OR EXISTS(SELECT 1 FROM resource_mediations m WHERE m.workspace_id=OLD.id
 AND m.status IN ('preparing','running'))
BEGIN SELECT RAISE(ABORT,'Workspace has unresolved resource coordination'); END;
CREATE TRIGGER resource_session_delete BEFORE DELETE ON sessions
WHEN EXISTS(SELECT 1 FROM resource_operations o WHERE o.session_id=OLD.id
 AND o.status IN ('queued','blocked','launching','running','recovery_required'))
 OR EXISTS(SELECT 1 FROM resource_mediations m WHERE m.session_id=OLD.id
 AND m.status IN ('preparing','running'))
BEGIN SELECT RAISE(ABORT,'Session has unresolved resource coordination'); END;
CREATE TRIGGER resource_process_delete BEFORE DELETE ON execution_processes
WHEN EXISTS(SELECT 1 FROM resource_operations o WHERE o.session_id=OLD.session_id
 AND o.status IN ('launching','running','recovery_required'))
BEGIN SELECT RAISE(ABORT,'Process evidence needed for resource recovery'); END;
