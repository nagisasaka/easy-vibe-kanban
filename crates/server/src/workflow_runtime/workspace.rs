use async_trait::async_trait;
use db::models::{
    scratch::{Scratch, ScratchPayload, ScratchType},
    workspace::{CreateWorkspace, Workspace, WorkspaceError},
    workspace_repo::{CreateWorkspaceRepo, WorkspaceRepo},
};
use deployment::Deployment;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use super::runner::{WorkflowWorkspaceRequest, WorkflowWorkspaceResolver};
use crate::{DeploymentImpl, error::ApiError};

/// Resolve the filesystem only at Agent dispatch, never while viewing/editing
/// an attempt. Internal executions are prepared by their owner and must not
/// silently recreate a missing worktree or repeat OpenWiki setup.
pub(super) async fn agent_workspace_path<F, Fut>(
    pool: &SqlitePool,
    workspace: &Workspace,
    ensure_interactive: F,
) -> Result<String, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<String, ApiError>>,
{
    if workspace.is_execution_only() {
        return services::services::workspace_usage::inspection_root(workspace)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| ApiError::BadRequest(format!("{error:#}")));
    }
    db::models::integration::guard_workspace(pool, workspace.id).await?;
    ensure_interactive().await
}

pub fn main_workflow_branch_name(issue_id: Uuid, run_id: Uuid) -> String {
    format!("vk/{issue_id}-wf-{}", short_run_id(run_id))
}

pub fn short_run_id(run_id: Uuid) -> String {
    run_id.simple().to_string()[..8].to_string()
}

async fn project_workspace_repos_from_db<F>(
    pool: &SqlitePool,
    project_id: Uuid,
    mut current_branch_for_path: F,
) -> Result<Vec<CreateWorkspaceRepo>, ApiError>
where
    F: FnMut(&str) -> String,
{
    let rows = sqlx::query(
        r#"
        SELECT r.id AS repo_id, r.path, r.default_target_branch
        FROM repos r
        JOIN project_repos pr ON pr.repo_id = r.id
        WHERE pr.project_id = ?
        ORDER BY r.display_name ASC
        "#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    if !rows.is_empty() {
        return rows
            .into_iter()
            .map(|row| {
                let repo_id: Uuid = row.get("repo_id");
                let path: String = row.get("path");
                let default_target_branch: Option<String> = row.get("default_target_branch");

                Ok::<CreateWorkspaceRepo, ApiError>(CreateWorkspaceRepo {
                    repo_id,
                    target_branch: resolve_target_branch(
                        default_target_branch.as_deref(),
                        &path,
                        &mut current_branch_for_path,
                    ),
                })
            })
            .collect();
    }

    let Some(scratch) =
        Scratch::find_by_id(pool, project_id, &ScratchType::ProjectRepoDefaults).await?
    else {
        return Ok(Vec::new());
    };

    let ScratchPayload::ProjectRepoDefaults(defaults) = scratch.payload else {
        return Ok(Vec::new());
    };

    let mut repos = Vec::with_capacity(defaults.repos.len());
    for draft_repo in defaults.repos {
        let row = sqlx::query(
            r#"
            SELECT id AS repo_id, path, default_target_branch
            FROM repos
            WHERE id = ?
            "#,
        )
        .bind(draft_repo.repo_id)
        .fetch_optional(pool)
        .await?;

        let Some(row) = row else {
            continue;
        };
        let repo_id: Uuid = row.get("repo_id");
        let path: String = row.get("path");
        let default_target_branch: Option<String> = row.get("default_target_branch");
        let target_branch = if draft_repo.target_branch.trim().is_empty() {
            resolve_target_branch(
                default_target_branch.as_deref(),
                &path,
                &mut current_branch_for_path,
            )
        } else {
            draft_repo.target_branch.trim().to_string()
        };

        repos.push(CreateWorkspaceRepo {
            repo_id,
            target_branch,
        });
    }

    Ok(repos)
}

fn resolve_target_branch<F>(
    preferred_branch: Option<&str>,
    path: &str,
    current_branch_for_path: &mut F,
) -> String
where
    F: FnMut(&str) -> String,
{
    preferred_branch
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| current_branch_for_path(path))
}

#[derive(Clone)]
pub struct DeploymentWorkflowWorkspaceResolver {
    deployment: DeploymentImpl,
}

impl DeploymentWorkflowWorkspaceResolver {
    pub fn new(deployment: DeploymentImpl) -> Self {
        Self { deployment }
    }

    async fn project_workspace_repos(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<CreateWorkspaceRepo>, ApiError> {
        project_workspace_repos_from_db(&self.deployment.db().pool, project_id, |path| {
            self.deployment
                .git()
                .get_current_branch(std::path::Path::new(path))
                .unwrap_or_else(|_| "main".to_string())
        })
        .await
    }
}

#[async_trait]
impl WorkflowWorkspaceResolver for DeploymentWorkflowWorkspaceResolver {
    async fn create_or_bind_main_workspace(
        &self,
        request: WorkflowWorkspaceRequest,
    ) -> Result<Uuid, ApiError> {
        let pool = &self.deployment.db().pool;

        if let Some(workspace_id) = request.existing_workspace_id {
            db::models::workspace_usage::require_interactive(pool, workspace_id).await?;
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspaces WHERE id = ?")
                .bind(workspace_id)
                .fetch_one(pool)
                .await?;
            if exists == 0 {
                return Err(ApiError::Workspace(WorkspaceError::WorkspaceNotFound));
            }

            if let Some(project_id) = request.project_id {
                upsert_workflow_workspace_link(pool, workspace_id, project_id, request.issue_id)
                    .await?;
            }

            return Ok(workspace_id);
        }

        let project_id = request.project_id.ok_or_else(|| {
            ApiError::BadRequest(
                "Workflow workspace creation requires a project with repositories. Select an existing workspace instead."
                    .to_string(),
            )
        })?;
        let repos = if request.repo_overrides.is_empty() {
            self.project_workspace_repos(project_id).await?
        } else {
            request.repo_overrides
        };
        if repos.is_empty() {
            return Err(ApiError::BadRequest(
                "Project has no repositories configured for workflow runs.".to_string(),
            ));
        }

        let workspace_id = Uuid::new_v4();
        let workspace = Workspace::create(
            pool,
            &CreateWorkspace {
                branch: request.branch_name,
                name: Some(format!("Workflow {}", short_run_id(request.run_id))),
            },
            workspace_id,
        )
        .await?;
        WorkspaceRepo::create_many(pool, workspace.id, &repos).await?;
        upsert_workflow_workspace_link(pool, workspace.id, project_id, request.issue_id).await?;

        Ok(workspace.id)
    }
}

async fn upsert_workflow_workspace_link(
    pool: &sqlx::SqlitePool,
    workspace_id: Uuid,
    project_id: Uuid,
    issue_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"INSERT INTO local_workspace_links
               (workspace_id, project_id, issue_id)
           VALUES (?, ?, ?)
           ON CONFLICT(workspace_id) DO UPDATE
               SET project_id = excluded.project_id,
                   issue_id = excluded.issue_id,
                   updated_at = datetime('now', 'subsec')"#,
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(issue_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
    use uuid::Uuid;

    use super::{project_workspace_repos_from_db, upsert_workflow_workspace_link};

    #[tokio::test]
    async fn agent_dispatch_materializes_a_lazy_workflow_workspace() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        let workspace = db::models::workspace::Workspace::create(
            &pool,
            &db::models::workspace::CreateWorkspace {
                branch: "fixture/workflow".into(),
                name: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap();
        assert!(workspace.container_ref.is_none());
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("prepared-workspace");
        let result = super::agent_workspace_path(&pool, &workspace, || async {
            std::fs::create_dir(&root).unwrap();
            let path = root.to_string_lossy().into_owned();
            db::models::workspace::Workspace::update_container_ref(&pool, workspace.id, &path)
                .await?;
            Ok(path)
        })
        .await
        .unwrap();
        assert_eq!(std::path::Path::new(&result), root);
        assert!(root.is_dir());
        assert_eq!(
            db::models::workspace::Workspace::find_by_id(&pool, workspace.id)
                .await
                .unwrap()
                .unwrap()
                .container_ref
                .as_deref(),
            Some(result.as_str())
        );

        // A subsequent dispatch delegates to the same idempotent container
        // service (including direct-folder validation), not a cached path.
        let loaded = db::models::workspace::Workspace::find_by_id(&pool, workspace.id)
            .await
            .unwrap()
            .unwrap();
        let called = std::cell::Cell::new(false);
        super::agent_workspace_path(&pool, &loaded, || async {
            called.set(true);
            Ok(result.clone())
        })
        .await
        .unwrap();
        assert!(called.get());
    }

    #[tokio::test]
    async fn agent_workspace_rejects_reserved_and_never_recreates_internal_execution() {
        use db::models::workspace::{CreateWorkspace, Workspace};
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        let mut workspace = Workspace::create(
            &pool,
            &CreateWorkspace {
                branch: "fixture/reserved".into(),
                name: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap();
        // The reservation's foreign-key owner is immaterial to this admission
        // check; keep this fixture focused on the actual guard query.
        sqlx::query("PRAGMA foreign_keys=OFF")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO integration_reservations(resource_kind,resource_key,run_id) VALUES ('workspace',?,?)")
            .bind(db::models::integration::resource_key(workspace.id)).bind(Uuid::new_v4())
            .execute(&pool).await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        workspace.container_ref = Some(temp.path().to_string_lossy().into_owned());
        let called = std::cell::Cell::new(false);
        assert!(
            super::agent_workspace_path(&pool, &workspace, || async {
                called.set(true);
                Ok("must-not-be-created".into())
            })
            .await
            .is_err()
        );
        assert!(!called.get());

        let mut internal = workspace.clone();
        internal.usage = db::models::workspace_usage::WorkspaceUsage::ExecutionOnly;
        internal.container_ref = None;
        assert!(
            super::agent_workspace_path(&pool, &internal, || async {
                called.set(true);
                Ok("must-not-be-recreated".into())
            })
            .await
            .is_err()
        );
        assert!(!called.get());
        internal.container_ref = Some(temp.path().to_string_lossy().into_owned());
        assert_eq!(
            super::agent_workspace_path(&pool, &internal, || async {
                called.set(true);
                Ok("must-not-run-setup".into())
            })
            .await
            .unwrap(),
            internal.container_ref.clone().unwrap()
        );
        assert!(!called.get());
        internal.worktree_deleted = true;
        assert!(
            super::agent_workspace_path(&pool, &internal, || async {
                called.set(true);
                Ok("must-not-revive".into())
            })
            .await
            .is_err()
        );
        assert!(!called.get());
    }

    async fn setup_repo_defaults_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");

        for statement in [
            r#"
            CREATE TABLE repos (
                id BLOB PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                display_name TEXT NOT NULL,
                default_target_branch TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec'))
            )
            "#,
            r#"
            CREATE TABLE project_repos (
                id BLOB PRIMARY KEY,
                project_id BLOB NOT NULL,
                repo_id BLOB NOT NULL,
                UNIQUE (project_id, repo_id)
            )
            "#,
            r#"
            CREATE TABLE scratch (
                id BLOB NOT NULL,
                scratch_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
                PRIMARY KEY (id, scratch_type)
            )
            "#,
        ] {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("create repo defaults schema");
        }

        pool
    }

    async fn insert_repo_defaults_scratch(
        pool: &SqlitePool,
        project_id: Uuid,
        repo_id: Uuid,
        target_branch: &str,
    ) {
        let payload = json!({
            "type": "PROJECT_REPO_DEFAULTS",
            "data": {
                "repos": [
                    {
                        "repo_id": repo_id,
                        "target_branch": target_branch
                    }
                ]
            }
        });

        sqlx::query(
            r#"
            INSERT INTO scratch (id, scratch_type, payload)
            VALUES (?, 'PROJECT_REPO_DEFAULTS', ?)
            "#,
        )
        .bind(project_id)
        .bind(payload.to_string())
        .execute(pool)
        .await
        .expect("insert project repo defaults scratch");
    }

    async fn insert_repo(
        pool: &SqlitePool,
        repo_id: Uuid,
        path: &str,
        display_name: &str,
        default_target_branch: Option<&str>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO repos (id, path, name, display_name, default_target_branch)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(repo_id)
        .bind(path)
        .bind(display_name)
        .bind(display_name)
        .bind(default_target_branch)
        .execute(pool)
        .await
        .expect("insert repo");
    }

    #[tokio::test]
    async fn project_workspace_repos_uses_project_repo_defaults_scratch_when_project_repos_empty() {
        let pool = setup_repo_defaults_pool().await;
        let project_id = Uuid::new_v4();
        let repo_id = Uuid::new_v4();
        insert_repo(&pool, repo_id, "F:\\Mydev2023\\repo", "repo", Some("main")).await;
        insert_repo_defaults_scratch(&pool, project_id, repo_id, "master").await;

        let repos = project_workspace_repos_from_db(&pool, project_id, |_| "fallback".to_string())
            .await
            .expect("resolve project workspace repos");

        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].repo_id, repo_id);
        assert_eq!(repos[0].target_branch, "master");
    }

    #[tokio::test]
    async fn upsert_workflow_workspace_link_makes_workspace_visible_for_issue() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::query(
            r#"
            CREATE TABLE local_workspace_links (
                workspace_id BLOB PRIMARY KEY,
                project_id BLOB NOT NULL,
                issue_id BLOB NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec'))
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create local workspace links");

        let workspace_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let issue_id = Uuid::new_v4();

        upsert_workflow_workspace_link(&pool, workspace_id, project_id, issue_id)
            .await
            .expect("upsert workflow workspace link");

        let linked_issue_id: Uuid =
            sqlx::query_scalar("SELECT issue_id FROM local_workspace_links WHERE workspace_id = ?")
                .bind(workspace_id)
                .fetch_one(&pool)
                .await
                .expect("fetch issue link");
        assert_eq!(linked_issue_id, issue_id);
    }
}
