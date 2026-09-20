-- Widen product/script discriminants without replacing referenced tables.
ALTER TABLE orchestration_runs ADD COLUMN product_kind_new TEXT NOT NULL
    DEFAULT 'workflow' CHECK(product_kind_new IN ('workflow','arena','integration'));
UPDATE orchestration_runs SET product_kind_new=product_kind;
DROP INDEX idx_orchestration_runs_source_definition;
ALTER TABLE orchestration_runs DROP COLUMN product_kind;
ALTER TABLE orchestration_runs RENAME COLUMN product_kind_new TO product_kind;
CREATE INDEX idx_orchestration_runs_source_definition ON orchestration_runs(product_kind,source_definition_id);

ALTER TABLE execution_processes ADD COLUMN run_reason_new TEXT NOT NULL DEFAULT 'setupscript'
    CHECK(run_reason_new IN ('setupscript','cleanupscript','archivescript','devserver','integrationvalidation'));
UPDATE execution_processes SET run_reason_new=run_reason;
DROP INDEX idx_execution_processes_run_reason;
DROP INDEX idx_execution_processes_session_status_run_reason;
DROP INDEX idx_execution_processes_session_run_reason_created;
ALTER TABLE execution_processes DROP COLUMN run_reason;
ALTER TABLE execution_processes RENAME COLUMN run_reason_new TO run_reason;
CREATE INDEX idx_execution_processes_run_reason ON execution_processes(run_reason);
CREATE INDEX idx_execution_processes_session_status_run_reason ON execution_processes(session_id,status,run_reason);
CREATE INDEX idx_execution_processes_session_run_reason_created ON execution_processes(session_id,run_reason,created_at DESC);

-- Product bookkeeping, not another scheduler. Agent commands/attempts/audit
-- remain in Orchestration; validation uses ordinary ExecutionProcesses.
CREATE TABLE integration_runs (
    id BLOB PRIMARY KEY,
    request_key TEXT NOT NULL UNIQUE,
    project_id BLOB NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    repository_id BLOB NOT NULL REFERENCES repos(id) ON DELETE RESTRICT,
    storage_identity TEXT NOT NULL,
    target_ref TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','preparing','integrating','validating','publishing','post_processing','succeeded','blocked','failed','cancelling','cancelled','recovery_required')),
    workspace_id BLOB REFERENCES workspaces(id) ON DELETE RESTRICT,
    session_id BLOB REFERENCES sessions(id) ON DELETE RESTRICT,
    agent_run_id BLOB,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT(datetime('now','subsec')),
    updated_at TEXT NOT NULL DEFAULT(datetime('now','subsec'))
);
CREATE INDEX integration_runs_status ON integration_runs(status,created_at);
-- These reservations are NOT dispatcher leases and have no expiry. Release
-- only after terminal process confirmation / publication recovery.
CREATE TABLE integration_reservations (
    resource_kind TEXT NOT NULL CHECK(resource_kind IN ('workspace','card','target')),
    resource_key TEXT NOT NULL,
    run_id BLOB NOT NULL REFERENCES integration_runs(id) ON DELETE RESTRICT,
    PRIMARY KEY(resource_kind,resource_key)
);
CREATE INDEX integration_reservations_run ON integration_reservations(run_id);

-- Revision is limited to requirements, status and adoption links. A timestamp
-- or display-only edit is not a requirements revision; change-and-revert still
-- invalidates a queued/completing adoption decision.
ALTER TABLE local_issues ADD COLUMN integration_revision INTEGER NOT NULL DEFAULT 0;
CREATE TRIGGER integration_issue_revision AFTER UPDATE OF title,description,status_id,project_id ON local_issues
WHEN NEW.title IS NOT OLD.title OR NEW.description IS NOT OLD.description OR NEW.status_id IS NOT OLD.status_id OR NEW.project_id IS NOT OLD.project_id
BEGIN UPDATE local_issues SET integration_revision=integration_revision+1 WHERE id=NEW.id; END;
CREATE TRIGGER integration_link_insert_revision AFTER INSERT ON local_workspace_links
BEGIN UPDATE local_issues SET integration_revision=integration_revision+1 WHERE id=NEW.issue_id; END;
CREATE TRIGGER integration_link_update_revision AFTER UPDATE OF issue_id,workspace_id ON local_workspace_links
WHEN NEW.issue_id IS NOT OLD.issue_id OR NEW.workspace_id IS NOT OLD.workspace_id
BEGIN UPDATE local_issues SET integration_revision=integration_revision+1 WHERE id=OLD.issue_id OR id=NEW.issue_id; END;
CREATE TRIGGER integration_link_delete_revision AFTER DELETE ON local_workspace_links
BEGIN UPDATE local_issues SET integration_revision=integration_revision+1 WHERE id=OLD.issue_id; END;

-- Admission and mutations race on the same SQLite writer. API guards provide
-- actionable run IDs; triggers are the last line of defence for other writers.
CREATE TRIGGER integration_agent_admission BEFORE INSERT ON agent_runs
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace'
 AND r.resource_key=lower(hex(NEW.workspace_id)) AND r.run_id<>NEW.correlation_id)
 OR EXISTS(SELECT 1 FROM integration_runs i WHERE i.workspace_id=NEW.workspace_id
 AND (i.id<>NEW.correlation_id OR i.session_id IS NOT NEW.session_id OR i.cancel_requested=1 OR i.status NOT IN ('preparing','integrating')))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_script_admission BEFORE INSERT ON execution_processes
WHEN EXISTS(SELECT 1 FROM integration_reservations r JOIN sessions s ON r.resource_key=lower(hex(s.workspace_id))
 WHERE r.resource_kind='workspace' AND s.id=NEW.session_id
 AND NOT EXISTS(SELECT 1 FROM integration_runs i WHERE i.id=r.run_id AND i.workspace_id=s.workspace_id
 AND i.session_id=s.id AND i.status='validating' AND NEW.run_reason='integrationvalidation'))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_session_admission BEFORE INSERT ON sessions
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace'
 AND r.resource_key=lower(hex(NEW.workspace_id)))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_card_update BEFORE UPDATE OF title,description,status_id,project_id ON local_issues
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='card' AND r.resource_key=lower(hex(OLD.id)))
BEGIN SELECT RAISE(ABORT,'Card reserved by formal Integration'); END;
CREATE TRIGGER integration_card_delete BEFORE DELETE ON local_issues
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='card' AND r.resource_key=lower(hex(OLD.id)))
BEGIN SELECT RAISE(ABORT,'Card reserved by formal Integration'); END;
CREATE TRIGGER integration_link_insert BEFORE INSERT ON local_workspace_links
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE (r.resource_kind='workspace' AND r.resource_key=lower(hex(NEW.workspace_id)))
 OR (r.resource_kind='card' AND r.resource_key=lower(hex(NEW.issue_id))))
BEGIN SELECT RAISE(ABORT,'Card or Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_link_update BEFORE UPDATE ON local_workspace_links
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE (r.resource_kind='workspace' AND r.resource_key IN (lower(hex(OLD.workspace_id)),lower(hex(NEW.workspace_id))))
 OR (r.resource_kind='card' AND r.resource_key IN (lower(hex(OLD.issue_id)),lower(hex(NEW.issue_id)))))
BEGIN SELECT RAISE(ABORT,'Card or Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_link_delete BEFORE DELETE ON local_workspace_links
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE (r.resource_kind='workspace' AND r.resource_key=lower(hex(OLD.workspace_id)))
 OR (r.resource_kind='card' AND r.resource_key=lower(hex(OLD.issue_id))))
BEGIN SELECT RAISE(ABORT,'Card or Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_workspace_delete BEFORE DELETE ON workspaces
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace' AND r.resource_key=lower(hex(OLD.id)))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_workspace_change BEFORE UPDATE OF branch,archived,worktree_deleted ON workspaces
WHEN (NEW.branch<>OLD.branch OR NEW.archived<>OLD.archived OR NEW.worktree_deleted<>OLD.worktree_deleted)
 AND EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace' AND r.resource_key=lower(hex(OLD.id)))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_workspace_repo_update BEFORE UPDATE ON workspace_repos
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace' AND r.resource_key IN (lower(hex(OLD.workspace_id)),lower(hex(NEW.workspace_id))))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_workspace_repo_delete BEFORE DELETE ON workspace_repos
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace' AND r.resource_key=lower(hex(OLD.workspace_id)))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_workspace_repo_insert BEFORE INSERT ON workspace_repos
WHEN EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace' AND r.resource_key=lower(hex(NEW.workspace_id)))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_attempt_admission BEFORE INSERT ON agent_run_attempts
WHEN EXISTS(SELECT 1 FROM agent_runs a JOIN integration_reservations r ON r.resource_key=lower(hex(a.workspace_id))
 WHERE r.resource_kind='workspace' AND a.id=NEW.agent_run_id AND r.run_id<>a.correlation_id)
 OR EXISTS(SELECT 1 FROM agent_runs a JOIN integration_runs i ON i.workspace_id=a.workspace_id
 WHERE a.id=NEW.agent_run_id AND (i.id<>a.correlation_id OR i.session_id IS NOT a.session_id OR i.cancel_requested=1 OR i.status NOT IN ('preparing','integrating')))
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
CREATE TRIGGER integration_agent_reactivation BEFORE UPDATE OF status ON agent_runs
WHEN NEW.status IN ('pending','starting','running','awaiting_input','awaiting_approval')
 AND EXISTS(SELECT 1 FROM integration_reservations r WHERE r.resource_kind='workspace'
 AND r.resource_key=lower(hex(NEW.workspace_id)) AND r.run_id<>NEW.correlation_id)
BEGIN SELECT RAISE(ABORT,'Workspace reserved by formal Integration'); END;
