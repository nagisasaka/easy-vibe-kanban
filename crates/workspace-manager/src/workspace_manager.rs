use std::path::{Path, PathBuf};

use db::{
    DBService,
    models::{
        file::WorkspaceAttachment,
        repo::{Repo, RepoError},
        requests::WorkspaceRepoInput,
        session::Session,
        workspace::Workspace as DbWorkspace,
        workspace_repo::{CreateWorkspaceRepo, RepoWithTargetBranch, WorkspaceRepo},
    },
};
use git::{GitService, GitServiceError};
use sqlx::{SqliteConnection, SqlitePool};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use worktree_manager::{WorktreeCleanup, WorktreeError, WorktreeManager};

#[derive(Debug, Clone)]
pub struct RepoWorkspaceInput {
    pub repo: Repo,
    pub target_branch: String,
}

impl RepoWorkspaceInput {
    pub fn new(repo: Repo, target_branch: String) -> Self {
        Self {
            repo,
            target_branch,
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
    #[error(transparent)]
    GitService(#[from] GitServiceError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Workspace not found")]
    WorkspaceNotFound,
    #[error("Repository already attached to workspace")]
    RepoAlreadyAttached,
    #[error("Branch '{branch}' does not exist in repository '{repo_name}'")]
    BranchNotFound { repo_name: String, branch: String },
    #[error("No repositories provided")]
    NoRepositories,
    #[error("Partial workspace creation failed: {0}")]
    PartialCreation(String),
    #[error("Workspace has {count} active agent run(s)")]
    ActiveAgentRuns { count: i64 },
    #[error("Workspace has {count} agent run(s) referenced by orchestration")]
    OrchestrationLinkedAgentRuns { count: i64 },
}

/// Info about a single repo's worktree within a workspace
#[derive(Debug, Clone)]
pub struct RepoWorktree {
    pub repo_id: Uuid,
    pub repo_name: String,
    pub source_repo_path: PathBuf,
    pub worktree_path: PathBuf,
}

/// A container directory holding worktrees for all project repos
#[derive(Debug, Clone)]
pub struct WorktreeContainer {
    pub workspace_dir: PathBuf,
    pub worktrees: Vec<RepoWorktree>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceDeletionContext {
    pub workspace_id: Uuid,
    pub branch_name: String,
    pub workspace_dir: Option<PathBuf>,
    pub repositories: Vec<Repo>,
    pub repo_paths: Vec<PathBuf>,
    pub session_ids: Vec<Uuid>,
}

#[derive(Clone)]
pub struct ManagedWorkspace {
    pub workspace: DbWorkspace,
    pub repos: Vec<RepoWithTargetBranch>,
    db: DBService,
}

impl ManagedWorkspace {
    fn new(db: DBService, workspace: DbWorkspace, repos: Vec<RepoWithTargetBranch>) -> Self {
        Self {
            workspace,
            repos,
            db,
        }
    }

    async fn attach_repository(&self, repo: &WorkspaceRepoInput) -> Result<(), sqlx::Error> {
        let create_repo = CreateWorkspaceRepo {
            repo_id: repo.repo_id,
            target_branch: repo.target_branch.clone(),
        };

        WorkspaceRepo::create_many(
            &self.db.pool,
            self.workspace.id,
            std::slice::from_ref(&create_repo),
        )
        .await
        .map(|_| ())
    }

    async fn refresh(&mut self) -> Result<(), WorkspaceError> {
        self.workspace = DbWorkspace::find_by_id(&self.db.pool, self.workspace.id)
            .await?
            .ok_or(WorkspaceError::WorkspaceNotFound)?;
        self.repos = WorkspaceRepo::find_repos_with_target_branch_for_workspace(
            &self.db.pool,
            self.workspace.id,
        )
        .await?;
        Ok(())
    }

    pub async fn add_repository(
        &mut self,
        repo_ref: &WorkspaceRepoInput,
        git: &GitService,
    ) -> Result<(), WorkspaceError> {
        let repo = Repo::find_by_id(&self.db.pool, repo_ref.repo_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        if !git.check_branch_exists(&repo.path, &repo_ref.target_branch)? {
            return Err(WorkspaceError::BranchNotFound {
                repo_name: repo.name,
                branch: repo_ref.target_branch.clone(),
            });
        }

        if WorkspaceRepo::find_by_workspace_and_repo_id(
            &self.db.pool,
            self.workspace.id,
            repo_ref.repo_id,
        )
        .await?
        .is_some()
        {
            return Err(WorkspaceError::RepoAlreadyAttached);
        }

        self.attach_repository(repo_ref).await?;
        self.refresh().await?;
        Ok(())
    }

    pub async fn associate_attachments(&self, attachment_ids: &[Uuid]) -> Result<(), sqlx::Error> {
        if attachment_ids.is_empty() {
            return Ok(());
        }

        WorkspaceAttachment::associate_many_dedup(&self.db.pool, self.workspace.id, attachment_ids)
            .await
    }

    pub async fn prepare_deletion_context(&self) -> Result<WorkspaceDeletionContext, sqlx::Error> {
        let repositories = if self.workspace.can_delete_container_path() {
            WorkspaceRepo::find_repos_for_workspace(&self.db.pool, self.workspace.id).await?
        } else {
            Vec::new()
        };
        let session_ids = Session::find_by_workspace_id(&self.db.pool, self.workspace.id)
            .await?
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();
        let repo_paths = if self.workspace.can_delete_container_path() {
            repositories
                .iter()
                .map(|repo| repo.path.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        Ok(WorkspaceDeletionContext {
            workspace_id: self.workspace.id,
            branch_name: self.workspace.branch.clone(),
            workspace_dir: self
                .workspace
                .can_delete_container_path()
                .then(|| self.workspace.container_ref.clone().map(PathBuf::from))
                .flatten(),
            repositories,
            repo_paths,
            session_ids,
        })
    }

    pub async fn delete_record(&self) -> Result<u64, WorkspaceError> {
        delete_workspace_record(&self.db.pool, self.workspace.id).await
    }
}

async fn delete_workspace_record(
    pool: &SqlitePool,
    workspace_id: Uuid,
) -> Result<u64, WorkspaceError> {
    let mut transaction = pool.begin().await?;

    let result = delete_workspace_record_in_transaction(&mut transaction, workspace_id).await;

    match result {
        Ok(rows_affected) => {
            transaction.commit().await?;
            Ok(rows_affected)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn delete_workspace_record_in_transaction(
    connection: &mut SqliteConnection,
    workspace_id: Uuid,
) -> Result<u64, WorkspaceError> {
    // Acquire SQLite's write lock before checking runtime state so that no agent
    // run can be inserted between the checks and the deletes below.
    let workspace_rows = sqlx::query("UPDATE workspaces SET updated_at = updated_at WHERE id = ?")
        .bind(workspace_id)
        .execute(&mut *connection)
        .await?
        .rows_affected();

    if workspace_rows == 0 {
        return Err(WorkspaceError::WorkspaceNotFound);
    }

    let active_agent_runs: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT agent_runs.id)
        FROM agent_runs
        WHERE agent_runs.workspace_id = ?
          AND (
            agent_runs.status NOT IN (
              'succeeded', 'failed', 'cancelled', 'crashed', 'audit_failed'
            )
            OR EXISTS (
              SELECT 1
              FROM agent_run_attempts
              JOIN agent_process_registry
                ON agent_process_registry.run_attempt_id = agent_run_attempts.id
              WHERE agent_run_attempts.agent_run_id = agent_runs.id
                AND agent_process_registry.registry_status IN (
                  'spawned', 'running', 'unreachable'
                )
            )
          )
        "#,
    )
    .bind(workspace_id)
    .fetch_one(&mut *connection)
    .await?;

    if active_agent_runs > 0 {
        return Err(WorkspaceError::ActiveAgentRuns {
            count: active_agent_runs,
        });
    }

    // A product deletion may have failed or predated Agent Runtime cleanup,
    // leaving a workflow orchestration aggregate with no owning workflow_run.
    // Once every AgentRun is terminal, that aggregate is safe to reclaim here.
    let orphaned_workflow_orchestrations: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT orchestration_runs.id
        FROM orchestration_runs
        JOIN orchestration_agent_run_links
          ON orchestration_agent_run_links.orchestration_run_id = orchestration_runs.id
        JOIN agent_runs
          ON agent_runs.id = orchestration_agent_run_links.agent_run_id
        WHERE agent_runs.workspace_id = ?
          AND orchestration_runs.product_kind = 'workflow'
          AND NOT EXISTS (
            SELECT 1
            FROM workflow_runs
            WHERE workflow_runs.orchestration_run_id = orchestration_runs.id
          )
          AND NOT EXISTS (
            SELECT 1
            FROM orchestration_agent_run_links AS linked_runs
            JOIN agent_runs AS linked_agent_runs
              ON linked_agent_runs.id = linked_runs.agent_run_id
            WHERE linked_runs.orchestration_run_id = orchestration_runs.id
              AND (
                linked_agent_runs.status NOT IN (
                  'succeeded', 'failed', 'cancelled', 'crashed', 'audit_failed'
                )
                OR EXISTS (
                  SELECT 1
                  FROM agent_run_attempts
                  JOIN agent_process_registry
                    ON agent_process_registry.run_attempt_id = agent_run_attempts.id
                  WHERE agent_run_attempts.agent_run_id = linked_agent_runs.id
                    AND agent_process_registry.registry_status IN (
                      'spawned', 'running', 'unreachable'
                    )
                )
              )
          )
        "#,
    )
    .bind(workspace_id)
    .fetch_all(&mut *connection)
    .await?;
    delete_orchestration_runs_in_transaction(connection, &orphaned_workflow_orchestrations).await?;

    let orchestration_links: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM orchestration_agent_run_links
        JOIN agent_runs
          ON agent_runs.id = orchestration_agent_run_links.agent_run_id
        WHERE agent_runs.workspace_id = ?
        "#,
    )
    .bind(workspace_id)
    .fetch_one(&mut *connection)
    .await?;

    if orchestration_links > 0 {
        return Err(WorkspaceError::OrchestrationLinkedAgentRuns {
            count: orchestration_links,
        });
    }

    // These deletes are intentionally explicit. The workspace foreign key on
    // agent_runs remains RESTRICT so accidental workspace deletion cannot erase
    // audit history without going through this lifecycle-aware path.
    sqlx::query("DELETE FROM agent_runs WHERE workspace_id = ?")
        .bind(workspace_id)
        .execute(&mut *connection)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE workspace_id = ?")
        .bind(workspace_id)
        .execute(&mut *connection)
        .await?;
    let rows_affected = sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(workspace_id)
        .execute(&mut *connection)
        .await?
        .rows_affected();

    Ok(rows_affected)
}

async fn delete_orchestration_runs_in_transaction(
    connection: &mut SqliteConnection,
    orchestration_run_ids: &[Uuid],
) -> Result<(), WorkspaceError> {
    for orchestration_run_id in orchestration_run_ids {
        let node_execution_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM orchestration_node_executions WHERE orchestration_run_id = ?",
        )
        .bind(orchestration_run_id)
        .fetch_all(&mut *connection)
        .await?;

        sqlx::query("DELETE FROM orchestration_leases WHERE resource_id = ?")
            .bind(orchestration_run_id)
            .execute(&mut *connection)
            .await?;
        for node_execution_id in node_execution_ids {
            sqlx::query("DELETE FROM orchestration_leases WHERE resource_id = ?")
                .bind(node_execution_id)
                .execute(&mut *connection)
                .await?;
        }

        // Keep the cleanup order explicit rather than relying solely on FK
        // cascades: consumption references links, events, and node executions.
        for table in [
            "orchestration_consumption",
            "orchestration_inbox",
            "orchestration_outbox",
            "orchestration_agent_run_links",
            "orchestration_state",
            "orchestration_events",
            "orchestration_node_executions",
        ] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE orchestration_run_id = ?"
            ))
            .bind(orchestration_run_id)
            .execute(&mut *connection)
            .await?;
        }
        sqlx::query("DELETE FROM orchestration_runs WHERE id = ?")
            .bind(orchestration_run_id)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct WorkspaceManager {
    db: DBService,
}

impl WorkspaceManager {
    pub fn new(db: DBService) -> Self {
        Self { db }
    }

    pub async fn load_managed_workspace(
        &self,
        workspace: DbWorkspace,
    ) -> Result<ManagedWorkspace, sqlx::Error> {
        let repos =
            WorkspaceRepo::find_repos_with_target_branch_for_workspace(&self.db.pool, workspace.id)
                .await?;
        Ok(ManagedWorkspace::new(self.db.clone(), workspace, repos))
    }

    pub fn spawn_workspace_deletion_cleanup(
        context: WorkspaceDeletionContext,
        delete_branches: bool,
    ) {
        tokio::spawn(async move {
            let WorkspaceDeletionContext {
                workspace_id,
                branch_name,
                workspace_dir,
                repositories,
                repo_paths,
                session_ids,
            } = context;

            for session_id in session_ids {
                if let Err(e) = Self::remove_session_runtime_files(session_id).await {
                    warn!(
                        "Failed to remove filesystem runtime files for session {}: {}",
                        session_id, e
                    );
                }
            }

            if let Some(workspace_dir) = workspace_dir {
                info!(
                    "Starting background cleanup for workspace {} at {}",
                    workspace_id,
                    workspace_dir.display()
                );

                if let Err(e) = Self::cleanup_workspace(&workspace_dir, &repositories).await {
                    error!(
                        "Background workspace cleanup failed for {} at {}: {}",
                        workspace_id,
                        workspace_dir.display(),
                        e
                    );
                } else {
                    info!(
                        "Background cleanup completed for workspace {}",
                        workspace_id
                    );
                }
            }

            if delete_branches {
                let git_service = GitService::new();
                for repo_path in repo_paths {
                    match git_service.delete_branch(&repo_path, &branch_name) {
                        Ok(()) => {
                            info!("Deleted branch '{}' from repo {:?}", branch_name, repo_path);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to delete branch '{}' from repo {:?}: {}",
                                branch_name, repo_path, e
                            );
                        }
                    }
                }
            }
        });
    }

    async fn remove_session_runtime_files(session_id: Uuid) -> Result<(), std::io::Error> {
        Self::remove_session_runtime_files_in_root(&utils::assets::asset_dir(), session_id).await
    }

    async fn remove_session_runtime_files_in_root(
        root: &Path,
        session_id: Uuid,
    ) -> Result<(), std::io::Error> {
        let process_logs =
            utils::execution_logs::process_logs_session_dir_in_root(root, session_id);
        let native_audit = utils::native_audit::session_dir_in_root(root, session_id);

        let process_logs_result = Self::remove_dir_if_exists(&process_logs).await;
        let native_audit_result = Self::remove_dir_if_exists(&native_audit).await;

        process_logs_result.and(native_audit_result)
    }

    async fn remove_dir_if_exists(dir: &Path) -> Result<(), std::io::Error> {
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Create a workspace with worktrees for all repositories.
    /// On failure, rolls back any already-created worktrees.
    pub async fn create_workspace(
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<WorktreeContainer, WorkspaceError> {
        if repos.is_empty() {
            return Err(WorkspaceError::NoRepositories);
        }

        info!(
            "Creating workspace at {} with {} repositories",
            workspace_dir.display(),
            repos.len()
        );

        tokio::fs::create_dir_all(workspace_dir).await?;

        let mut created_worktrees: Vec<RepoWorktree> = Vec::new();

        for input in repos {
            let worktree_path = workspace_dir.join(&input.repo.name);

            debug!(
                "Creating worktree for repo '{}' at {}",
                input.repo.name,
                worktree_path.display()
            );

            match WorktreeManager::create_worktree(
                &input.repo.path,
                branch_name,
                &worktree_path,
                &input.target_branch,
                true,
            )
            .await
            {
                Ok(()) => {
                    created_worktrees.push(RepoWorktree {
                        repo_id: input.repo.id,
                        repo_name: input.repo.name.clone(),
                        source_repo_path: input.repo.path.clone(),
                        worktree_path,
                    });
                }
                Err(e) => {
                    error!(
                        "Failed to create worktree for repo '{}': {}. Rolling back...",
                        input.repo.name, e
                    );

                    // Rollback: cleanup all worktrees we've created so far
                    Self::cleanup_created_worktrees(&created_worktrees).await;

                    // Also remove the workspace directory if it's empty
                    if let Err(cleanup_err) = tokio::fs::remove_dir(workspace_dir).await {
                        debug!(
                            "Could not remove workspace dir during rollback: {}",
                            cleanup_err
                        );
                    }

                    return Err(WorkspaceError::PartialCreation(format!(
                        "Failed to create worktree for repo '{}': {}",
                        input.repo.name, e
                    )));
                }
            }
        }

        info!(
            "Successfully created workspace with {} worktrees",
            created_worktrees.len()
        );

        Ok(WorktreeContainer {
            workspace_dir: workspace_dir.to_path_buf(),
            worktrees: created_worktrees,
        })
    }

    /// Ensure all worktrees in a workspace exist (for cold restart scenarios)
    pub async fn ensure_workspace_exists(
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<(), WorkspaceError> {
        if repos.is_empty() {
            return Err(WorkspaceError::NoRepositories);
        }

        // Try legacy migration first (single repo projects only)
        // Old layout had worktree directly at workspace_dir; new layout has it at workspace_dir/{repo_name}
        if repos.len() == 1 && Self::migrate_legacy_worktree(workspace_dir, &repos[0].repo).await? {
            return Ok(());
        }

        if !workspace_dir.exists() {
            tokio::fs::create_dir_all(workspace_dir).await?;
        }

        let git = GitService::new();

        for input in repos {
            let repo = &input.repo;
            let worktree_path = workspace_dir.join(&repo.name);

            debug!(
                "Ensuring worktree exists for repo '{}' at {}",
                repo.name,
                worktree_path.display()
            );

            if git.check_branch_exists(&repo.path, branch_name)? {
                WorktreeManager::ensure_worktree_exists(&repo.path, branch_name, &worktree_path)
                    .await?;
            } else {
                info!(
                    "Workspace branch '{}' missing in repo '{}'; creating from target branch '{}'",
                    branch_name, repo.name, input.target_branch
                );
                WorktreeManager::create_worktree(
                    &repo.path,
                    branch_name,
                    &worktree_path,
                    &input.target_branch,
                    true,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Clean up all worktrees in a workspace
    pub async fn cleanup_workspace(
        workspace_dir: &Path,
        repos: &[Repo],
    ) -> Result<(), WorkspaceError> {
        info!("Cleaning up workspace at {}", workspace_dir.display());

        let cleanup_data: Vec<WorktreeCleanup> = repos
            .iter()
            .map(|repo| {
                let worktree_path = workspace_dir.join(&repo.name);
                WorktreeCleanup::new(worktree_path, Some(repo.path.clone()))
            })
            .collect();

        WorktreeManager::batch_cleanup_worktrees(&cleanup_data).await?;

        // Remove the workspace directory itself
        if workspace_dir.exists()
            && let Err(e) = tokio::fs::remove_dir_all(workspace_dir).await
        {
            debug!(
                "Could not remove workspace directory {}: {}",
                workspace_dir.display(),
                e
            );
        }

        Ok(())
    }

    /// Get the base directory for workspaces (same as worktree base dir)
    pub fn get_workspace_base_dir() -> PathBuf {
        WorktreeManager::get_worktree_base_dir()
    }

    /// Migrate a legacy single-worktree layout to the new workspace layout.
    /// Old layout: workspace_dir IS the worktree
    /// New layout: workspace_dir contains worktrees at workspace_dir/{repo_name}
    ///
    /// Returns Ok(true) if migration was performed, Ok(false) if no migration needed.
    async fn migrate_legacy_worktree(
        workspace_dir: &Path,
        repo: &Repo,
    ) -> Result<bool, WorkspaceError> {
        let expected_worktree_path = workspace_dir.join(&repo.name);

        // Detect old-style: workspace_dir exists AND has .git file (worktree marker)
        // AND expected new location doesn't exist
        let git_file = workspace_dir.join(".git");
        let is_old_style = workspace_dir.exists()
            && git_file.exists()
            && git_file.is_file() // .git file = worktree, .git dir = main repo
            && !expected_worktree_path.exists();

        if !is_old_style {
            return Ok(false);
        }

        info!(
            "Detected legacy worktree at {}, migrating to new layout",
            workspace_dir.display()
        );

        // Move old worktree to temp location (can't move into subdirectory of itself)
        let temp_name = format!(
            "{}-migrating",
            workspace_dir
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        );
        let temp_path = workspace_dir.with_file_name(temp_name);

        WorktreeManager::move_worktree(&repo.path, workspace_dir, &temp_path).await?;

        // Create new workspace directory
        tokio::fs::create_dir_all(workspace_dir).await?;

        // Move worktree to final location using git worktree move
        WorktreeManager::move_worktree(&repo.path, &temp_path, &expected_worktree_path).await?;

        if temp_path.exists() {
            let _ = tokio::fs::remove_dir_all(&temp_path).await;
        }

        info!(
            "Successfully migrated legacy worktree to {}",
            expected_worktree_path.display()
        );

        Ok(true)
    }

    /// Helper to cleanup worktrees during rollback
    async fn cleanup_created_worktrees(worktrees: &[RepoWorktree]) {
        for worktree in worktrees {
            let cleanup = WorktreeCleanup::new(
                worktree.worktree_path.clone(),
                Some(worktree.source_repo_path.clone()),
            );

            if let Err(e) = WorktreeManager::cleanup_worktree(&cleanup).await {
                error!(
                    "Failed to cleanup worktree '{}' during rollback: {}",
                    worktree.repo_name, e
                );
            }
        }
    }

    pub async fn cleanup_orphan_workspaces(&self) {
        if std::env::var("DISABLE_WORKTREE_CLEANUP").is_ok() {
            info!(
                "Orphan workspace cleanup is disabled via DISABLE_WORKTREE_CLEANUP environment variable"
            );
            return;
        }

        // Always clean up the default directory
        let default_dir = WorktreeManager::get_default_worktree_base_dir();
        self.cleanup_orphans_in_directory(&default_dir).await;

        // Also clean up custom directory if it's different from the default
        let current_dir = Self::get_workspace_base_dir();
        if current_dir != default_dir {
            self.cleanup_orphans_in_directory(&current_dir).await;
        }
    }

    async fn cleanup_orphans_in_directory(&self, workspace_base_dir: &Path) {
        if !workspace_base_dir.exists() {
            debug!(
                "Workspace base directory {} does not exist, skipping orphan cleanup",
                workspace_base_dir.display()
            );
            return;
        }

        let entries = match std::fs::read_dir(workspace_base_dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!(
                    "Failed to read workspace base directory {}: {}",
                    workspace_base_dir.display(),
                    e
                );
                return;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let workspace_path_str = path.to_string_lossy().to_string();
            if let Ok(false) =
                DbWorkspace::container_ref_exists(&self.db.pool, &workspace_path_str).await
            {
                info!("Found orphaned workspace: {}", workspace_path_str);
                if let Err(e) = Self::cleanup_workspace_without_repos(&path).await {
                    error!(
                        "Failed to remove orphaned workspace {}: {}",
                        workspace_path_str, e
                    );
                } else {
                    info!(
                        "Successfully removed orphaned workspace: {}",
                        workspace_path_str
                    );
                }
            }
        }
    }

    async fn cleanup_workspace_without_repos(workspace_dir: &Path) -> Result<(), WorkspaceError> {
        info!(
            "Cleaning up orphaned workspace at {}",
            workspace_dir.display()
        );

        let entries = match std::fs::read_dir(workspace_dir) {
            Ok(entries) => entries,
            Err(e) => {
                debug!(
                    "Cannot read workspace directory {}, attempting direct removal: {}",
                    workspace_dir.display(),
                    e
                );
                return tokio::fs::remove_dir_all(workspace_dir)
                    .await
                    .map_err(WorkspaceError::Io);
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir()
                && let Err(e) = WorktreeManager::cleanup_suspected_worktree(&path).await
            {
                warn!("Failed to cleanup suspected worktree: {}", e);
            }
        }

        if workspace_dir.exists()
            && let Err(e) = tokio::fs::remove_dir_all(workspace_dir).await
        {
            debug!(
                "Could not remove workspace directory {}: {}",
                workspace_dir.display(),
                e
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
    use uuid::Uuid;

    use super::{WorkspaceError, WorkspaceManager, delete_workspace_record};

    async fn runtime_test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect test database");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("enable foreign keys");
        sqlx::query(
            r#"
            CREATE TABLE workspaces (
                id BLOB PRIMARY KEY,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE sessions (
                id BLOB PRIMARY KEY,
                workspace_id BLOB NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );
            CREATE TABLE agent_runs (
                id BLOB PRIMARY KEY,
                session_id BLOB NOT NULL,
                workspace_id BLOB NOT NULL,
                status TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE RESTRICT
            );
            CREATE TABLE agent_run_attempts (
                id BLOB PRIMARY KEY,
                agent_run_id BLOB NOT NULL,
                FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id) ON DELETE CASCADE
            );
            CREATE TABLE agent_process_registry (
                id BLOB PRIMARY KEY,
                run_attempt_id BLOB NOT NULL,
                registry_status TEXT NOT NULL,
                FOREIGN KEY (run_attempt_id) REFERENCES agent_run_attempts(id) ON DELETE CASCADE
            );
            CREATE TABLE agent_runtime_children (
                id BLOB PRIMARY KEY,
                agent_run_id BLOB NOT NULL,
                FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id) ON DELETE CASCADE
            );
            CREATE TABLE orchestration_runs (
                id BLOB PRIMARY KEY,
                product_kind TEXT NOT NULL
            );
            CREATE TABLE workflow_runs (
                id BLOB PRIMARY KEY,
                orchestration_run_id BLOB,
                FOREIGN KEY (orchestration_run_id) REFERENCES orchestration_runs(id) ON DELETE SET NULL
            );
            CREATE TABLE orchestration_leases (
                resource_kind TEXT NOT NULL,
                resource_id BLOB NOT NULL,
                PRIMARY KEY (resource_kind, resource_id)
            );
            CREATE TABLE orchestration_node_executions (
                id BLOB PRIMARY KEY,
                orchestration_run_id BLOB NOT NULL
            );
            CREATE TABLE orchestration_consumption (
                id BLOB PRIMARY KEY,
                orchestration_run_id BLOB NOT NULL
            );
            CREATE TABLE orchestration_inbox (
                id BLOB PRIMARY KEY,
                orchestration_run_id BLOB NOT NULL
            );
            CREATE TABLE orchestration_outbox (
                id BLOB PRIMARY KEY,
                orchestration_run_id BLOB NOT NULL
            );
            CREATE TABLE orchestration_state (
                orchestration_run_id BLOB PRIMARY KEY
            );
            CREATE TABLE orchestration_events (
                event_id BLOB PRIMARY KEY,
                orchestration_run_id BLOB NOT NULL
            );
            CREATE TABLE orchestration_agent_run_links (
                id BLOB PRIMARY KEY,
                orchestration_run_id BLOB NOT NULL,
                agent_run_id BLOB NOT NULL,
                FOREIGN KEY (orchestration_run_id) REFERENCES orchestration_runs(id) ON DELETE CASCADE,
                FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id) ON DELETE RESTRICT
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create runtime test schema");
        pool
    }

    async fn seed_workspace(pool: &SqlitePool) -> (Uuid, Uuid) {
        let workspace_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        sqlx::query("INSERT INTO workspaces (id) VALUES (?)")
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("insert workspace");
        sqlx::query("INSERT INTO sessions (id, workspace_id) VALUES (?, ?)")
            .bind(session_id)
            .bind(workspace_id)
            .execute(pool)
            .await
            .expect("insert session");
        (workspace_id, session_id)
    }

    async fn seed_agent_run(
        pool: &SqlitePool,
        workspace_id: Uuid,
        session_id: Uuid,
        status: &str,
    ) -> Uuid {
        let agent_run_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_runs (id, session_id, workspace_id, status) VALUES (?, ?, ?, ?)",
        )
        .bind(agent_run_id)
        .bind(session_id)
        .bind(workspace_id)
        .bind(status)
        .execute(pool)
        .await
        .expect("insert agent run");
        agent_run_id
    }

    async fn count(pool: &SqlitePool, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .expect("count table")
    }

    #[tokio::test]
    async fn deletes_workspace_without_agent_runs() {
        let pool = runtime_test_pool().await;
        let (workspace_id, _) = seed_workspace(&pool).await;

        let rows = delete_workspace_record(&pool, workspace_id)
            .await
            .expect("delete workspace");

        assert_eq!(rows, 1);
        assert_eq!(count(&pool, "workspaces").await, 0);
        assert_eq!(count(&pool, "sessions").await, 0);
    }

    #[tokio::test]
    async fn explicitly_deletes_terminal_agent_runtime_graph() {
        let pool = runtime_test_pool().await;
        let (workspace_id, session_id) = seed_workspace(&pool).await;
        let agent_run_id = seed_agent_run(&pool, workspace_id, session_id, "succeeded").await;
        sqlx::query("INSERT INTO agent_runtime_children (id, agent_run_id) VALUES (?, ?)")
            .bind(Uuid::new_v4())
            .bind(agent_run_id)
            .execute(&pool)
            .await
            .expect("insert runtime child");

        delete_workspace_record(&pool, workspace_id)
            .await
            .expect("delete terminal runtime graph");

        for table in [
            "agent_runtime_children",
            "agent_runs",
            "sessions",
            "workspaces",
        ] {
            assert_eq!(count(&pool, table).await, 0, "{table} was not deleted");
        }
        let foreign_key_violations: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .expect("check foreign keys");
        assert_eq!(foreign_key_violations, 0);
    }

    #[tokio::test]
    async fn deletes_terminal_runtime_graph_with_production_migrations() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect production-schema test database");
        sqlx::migrate!("../db/migrations")
            .run(&pool)
            .await
            .expect("run production migrations");

        let workspace_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let agent_run_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let attempt_id = Uuid::new_v4();
        let orchestration_run_id = Uuid::new_v4();
        let orchestration_node_id = Uuid::new_v4();
        sqlx::query("INSERT INTO workspaces (id, branch) VALUES (?, 'test-branch')")
            .bind(workspace_id)
            .execute(&pool)
            .await
            .expect("insert workspace");
        sqlx::query("INSERT INTO sessions (id, workspace_id) VALUES (?, ?)")
            .bind(session_id)
            .bind(workspace_id)
            .execute(&pool)
            .await
            .expect("insert session");
        sqlx::query(
            r#"
            INSERT INTO agent_runs (
                id, session_id, workspace_id, request_id, idempotency_key,
                correlation_id, schema_version, payload_version,
                runtime_profile_id, provider_id, workspace_mode,
                workspace_path, status, request_envelope
            ) VALUES (?, ?, ?, ?, ?, ?, 1, 1, 'test-profile', 'codex',
                      'shared_workspace', '/tmp/test-workspace', 'succeeded', '{}')
            "#,
        )
        .bind(agent_run_id)
        .bind(session_id)
        .bind(workspace_id)
        .bind(Uuid::new_v4())
        .bind(format!("run-{agent_run_id}"))
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .expect("insert agent run");
        sqlx::query(
            r#"
            INSERT INTO agent_turns (
                id, agent_run_id, request_id, turn_number, intent, input_message
            ) VALUES (?, ?, ?, 1, 'initial', '{}')
            "#,
        )
        .bind(turn_id)
        .bind(agent_run_id)
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .expect("insert agent turn");
        sqlx::query(
            r#"
            INSERT INTO agent_run_attempts (
                id, agent_run_id, turn_id, request_id, idempotency_key,
                attempt_number, mode, transport, schema_version, payload_version,
                capability_snapshot, request_envelope, status
            ) VALUES (?, ?, ?, ?, ?, 1, 'launch', 'app_server_jsonrpc', 1, 1,
                      '{}', '{}', 'succeeded')
            "#,
        )
        .bind(attempt_id)
        .bind(agent_run_id)
        .bind(turn_id)
        .bind(Uuid::new_v4())
        .bind(format!("attempt-{attempt_id}"))
        .execute(&pool)
        .await
        .expect("insert run attempt");
        sqlx::query(
            r#"
            INSERT INTO native_audit_streams (
                id, session_id, agent_run_id, run_attempt_id,
                audit_schema_version, adapter_version, mapper_version,
                manifest_relative_path, frames_relative_path, integrity_status
            ) VALUES (?, ?, ?, ?, 1, 'test-adapter', 'test-mapper', ?, ?, 'complete')
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(session_id)
        .bind(agent_run_id)
        .bind(attempt_id)
        .bind(format!("manifest-{attempt_id}.json"))
        .bind(format!("frames-{attempt_id}.jsonl"))
        .execute(&pool)
        .await
        .expect("insert native audit stream");
        sqlx::query(
            r#"
            INSERT INTO orchestration_runs (
                id, request_id, idempotency_key, correlation_id, product_kind,
                source_definition_id, source_definition_version,
                plan_schema_version, plan_snapshot, status
            ) VALUES (?, ?, ?, ?, 'workflow', ?, 'test', 1, '{}', 'running')
            "#,
        )
        .bind(orchestration_run_id)
        .bind(Uuid::new_v4())
        .bind(format!("orchestration-{orchestration_run_id}"))
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .expect("insert orphaned orchestration run");
        sqlx::query(
            r#"
            INSERT INTO orchestration_node_executions (
                id, orchestration_run_id, node_key, stable_order, status
            ) VALUES (?, ?, 'agent', 1, 'succeeded')
            "#,
        )
        .bind(orchestration_node_id)
        .bind(orchestration_run_id)
        .execute(&pool)
        .await
        .expect("insert orchestration node");
        sqlx::query(
            r#"
            INSERT INTO orchestration_agent_run_links (
                id, orchestration_run_id, node_execution_id,
                agent_run_id, dispatch_idempotency_key
            ) VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(orchestration_run_id)
        .bind(orchestration_node_id)
        .bind(agent_run_id)
        .bind(format!("dispatch-{agent_run_id}"))
        .execute(&pool)
        .await
        .expect("link orphaned orchestration run");
        for (resource_kind, resource_id) in [
            ("reconciler", orchestration_run_id),
            ("each_queue", orchestration_node_id),
        ] {
            sqlx::query(
                r#"
                INSERT INTO orchestration_leases (
                    resource_kind, resource_id, owner_id, fencing_token,
                    acquired_at, expires_at, updated_at
                ) VALUES (?, ?, 'test-owner', 1, datetime('now'), datetime('now', '+1 hour'), datetime('now'))
                "#,
            )
            .bind(resource_kind)
            .bind(resource_id)
            .execute(&pool)
            .await
            .expect("insert orchestration lease");
        }

        delete_workspace_record(&pool, workspace_id)
            .await
            .expect("delete production runtime graph");

        for table in [
            "orchestration_leases",
            "orchestration_agent_run_links",
            "orchestration_node_executions",
            "orchestration_runs",
            "native_audit_streams",
            "agent_run_attempts",
            "agent_turns",
            "agent_runs",
            "sessions",
            "workspaces",
        ] {
            assert_eq!(count(&pool, table).await, 0, "{table} was not deleted");
        }
        let foreign_key_violations: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .expect("check production foreign keys");
        assert_eq!(foreign_key_violations, 0);
    }

    #[tokio::test]
    async fn rejects_active_run_and_terminal_run_with_live_process() {
        for (status, registry_status) in [("running", None), ("succeeded", Some("unreachable"))] {
            let pool = runtime_test_pool().await;
            let (workspace_id, session_id) = seed_workspace(&pool).await;
            let agent_run_id = seed_agent_run(&pool, workspace_id, session_id, status).await;
            if let Some(registry_status) = registry_status {
                let attempt_id = Uuid::new_v4();
                sqlx::query("INSERT INTO agent_run_attempts (id, agent_run_id) VALUES (?, ?)")
                    .bind(attempt_id)
                    .bind(agent_run_id)
                    .execute(&pool)
                    .await
                    .expect("insert attempt");
                sqlx::query(
                    "INSERT INTO agent_process_registry (id, run_attempt_id, registry_status) VALUES (?, ?, ?)",
                )
                .bind(Uuid::new_v4())
                .bind(attempt_id)
                .bind(registry_status)
                .execute(&pool)
                .await
                .expect("insert process registry");
            }

            let error = delete_workspace_record(&pool, workspace_id)
                .await
                .expect_err("active runtime must block deletion");
            assert!(matches!(
                error,
                WorkspaceError::ActiveAgentRuns { count: 1 }
            ));
            assert_eq!(count(&pool, "workspaces").await, 1);
            assert_eq!(count(&pool, "agent_runs").await, 1);
        }
    }

    #[tokio::test]
    async fn rejects_orchestration_linked_agent_run() {
        let pool = runtime_test_pool().await;
        let (workspace_id, session_id) = seed_workspace(&pool).await;
        let agent_run_id = seed_agent_run(&pool, workspace_id, session_id, "failed").await;
        let orchestration_run_id = Uuid::new_v4();
        sqlx::query("INSERT INTO orchestration_runs (id, product_kind) VALUES (?, 'workflow')")
            .bind(orchestration_run_id)
            .execute(&pool)
            .await
            .expect("insert orchestration run");
        sqlx::query("INSERT INTO workflow_runs (id, orchestration_run_id) VALUES (?, ?)")
            .bind(Uuid::new_v4())
            .bind(orchestration_run_id)
            .execute(&pool)
            .await
            .expect("retain orchestration from workflow");
        sqlx::query(
            "INSERT INTO orchestration_agent_run_links (id, orchestration_run_id, agent_run_id) VALUES (?, ?, ?)",
        )
            .bind(Uuid::new_v4())
            .bind(orchestration_run_id)
            .bind(agent_run_id)
            .execute(&pool)
            .await
            .expect("insert orchestration link");

        let error = delete_workspace_record(&pool, workspace_id)
            .await
            .expect_err("orchestration link must block deletion");

        assert!(matches!(
            error,
            WorkspaceError::OrchestrationLinkedAgentRuns { count: 1 }
        ));
        assert_eq!(count(&pool, "workspaces").await, 1);
        assert_eq!(count(&pool, "agent_runs").await, 1);
    }

    #[tokio::test]
    async fn deletes_orphaned_workflow_orchestration_before_workspace() {
        let pool = runtime_test_pool().await;
        let (workspace_id, session_id) = seed_workspace(&pool).await;
        let agent_run_id = seed_agent_run(&pool, workspace_id, session_id, "succeeded").await;
        let orchestration_run_id = Uuid::new_v4();
        sqlx::query("INSERT INTO orchestration_runs (id, product_kind) VALUES (?, 'workflow')")
            .bind(orchestration_run_id)
            .execute(&pool)
            .await
            .expect("insert orphaned orchestration run");
        sqlx::query(
            "INSERT INTO orchestration_agent_run_links (id, orchestration_run_id, agent_run_id) VALUES (?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(orchestration_run_id)
        .bind(agent_run_id)
        .execute(&pool)
        .await
        .expect("link orphaned orchestration run");
        sqlx::query(
            "INSERT INTO orchestration_leases (resource_kind, resource_id) VALUES ('reconciler', ?)",
        )
        .bind(orchestration_run_id)
        .execute(&pool)
        .await
        .expect("insert orphaned orchestration lease");

        delete_workspace_record(&pool, workspace_id)
            .await
            .expect("delete workspace with orphaned workflow orchestration");

        assert_eq!(count(&pool, "orchestration_agent_run_links").await, 0);
        assert_eq!(count(&pool, "orchestration_leases").await, 0);
        assert_eq!(count(&pool, "orchestration_runs").await, 0);
        assert_eq!(count(&pool, "agent_runs").await, 0);
        assert_eq!(count(&pool, "workspaces").await, 0);
    }

    #[tokio::test]
    async fn retains_orphaned_orchestration_linked_to_an_active_run_elsewhere() {
        let pool = runtime_test_pool().await;
        let (target_workspace_id, target_session_id) = seed_workspace(&pool).await;
        let target_agent_run_id =
            seed_agent_run(&pool, target_workspace_id, target_session_id, "succeeded").await;
        let (active_workspace_id, active_session_id) = seed_workspace(&pool).await;
        let active_agent_run_id =
            seed_agent_run(&pool, active_workspace_id, active_session_id, "running").await;
        let orchestration_run_id = Uuid::new_v4();
        sqlx::query("INSERT INTO orchestration_runs (id, product_kind) VALUES (?, 'workflow')")
            .bind(orchestration_run_id)
            .execute(&pool)
            .await
            .expect("insert shared orphaned orchestration run");
        for agent_run_id in [target_agent_run_id, active_agent_run_id] {
            sqlx::query(
                "INSERT INTO orchestration_agent_run_links (id, orchestration_run_id, agent_run_id) VALUES (?, ?, ?)",
            )
            .bind(Uuid::new_v4())
            .bind(orchestration_run_id)
            .bind(agent_run_id)
            .execute(&pool)
            .await
            .expect("insert shared orchestration link");
        }

        let error = delete_workspace_record(&pool, target_workspace_id)
            .await
            .expect_err("active linked run must preserve the orchestration aggregate");

        assert!(matches!(
            error,
            WorkspaceError::OrchestrationLinkedAgentRuns { count: 1 }
        ));
        assert_eq!(count(&pool, "orchestration_runs").await, 1);
        assert_eq!(count(&pool, "orchestration_agent_run_links").await, 2);
        assert_eq!(count(&pool, "agent_runs").await, 2);
        assert_eq!(count(&pool, "workspaces").await, 2);
    }

    #[tokio::test]
    async fn rolls_back_if_an_unknown_restrict_reference_blocks_cleanup() {
        let pool = runtime_test_pool().await;
        sqlx::query(
            r#"
            CREATE TABLE retained_runtime_reference (
                id BLOB PRIMARY KEY,
                agent_run_id BLOB NOT NULL,
                FOREIGN KEY (agent_run_id) REFERENCES agent_runs(id) ON DELETE RESTRICT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create retained reference");
        let (workspace_id, session_id) = seed_workspace(&pool).await;
        let agent_run_id = seed_agent_run(&pool, workspace_id, session_id, "succeeded").await;
        sqlx::query("INSERT INTO retained_runtime_reference (id, agent_run_id) VALUES (?, ?)")
            .bind(Uuid::new_v4())
            .bind(agent_run_id)
            .execute(&pool)
            .await
            .expect("insert retained reference");

        delete_workspace_record(&pool, workspace_id)
            .await
            .expect_err("restrict reference must fail transaction");

        assert_eq!(count(&pool, "workspaces").await, 1);
        assert_eq!(count(&pool, "sessions").await, 1);
        assert_eq!(count(&pool, "agent_runs").await, 1);
    }

    #[tokio::test]
    async fn removes_process_logs_and_native_audit_files() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let session_id = Uuid::new_v4();
        let process_logs =
            utils::execution_logs::process_logs_session_dir_in_root(temp.path(), session_id);
        let native_audit = utils::native_audit::session_dir_in_root(temp.path(), session_id);
        tokio::fs::create_dir_all(&process_logs)
            .await
            .expect("create process logs");
        tokio::fs::create_dir_all(&native_audit)
            .await
            .expect("create native audit");
        tokio::fs::write(process_logs.join("process.jsonl"), b"log")
            .await
            .expect("write process log");
        tokio::fs::write(native_audit.join("frames.jsonl"), b"audit")
            .await
            .expect("write native audit");

        WorkspaceManager::remove_session_runtime_files_in_root(temp.path(), session_id)
            .await
            .expect("remove session runtime files");

        assert!(!process_logs.exists());
        assert!(!native_audit.exists());
        WorkspaceManager::remove_session_runtime_files_in_root(temp.path(), session_id)
            .await
            .expect("missing paths are idempotent");
    }
}
