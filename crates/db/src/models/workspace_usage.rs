//! Persistent human usage and execution attribution. This is not a scheduler or
//! an authority token: the owner's runtime still validates every dispatch.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, Type, types::Json};
use ts_rs::TS;
use uuid::Uuid;

use super::workspace::{Workspace, WorkspaceError};

pub const OPENWIKI_BOOTSTRAP: &str = "openwiki_bootstrap";
pub const OPENWIKI_SYNC: &str = "openwiki_sync";
pub const INTEGRATION: &str = "integration";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Type, Serialize, Deserialize, TS)]
#[sqlx(type_name = "workspace_usage", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum WorkspaceUsage {
    #[default]
    Interactive,
    ExecutionOnly,
}

/// An extensible reference to an existing product execution. Unknown kinds are
/// retained for inspection; they never acquire default dispatch privileges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceExecutionOwner {
    pub kind: String,
    pub run_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
    /// Only for preparation/maintenance results not already retained by the
    /// owner's product record. Never infer publication from AgentRun success.
    #[serde(default)]
    pub result: Option<WorkspaceExecutionResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceExecutionResult {
    pub status: WorkspaceExecutionTerminalStatus,
    pub completed_at: DateTime<Utc>,
    pub error: Option<String>,
    pub wiki_commit: Option<String>,
    pub no_op: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceExecutionTerminalStatus {
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkspaceExecutionOwner {
    pub fn new(kind: &str, repository_id: Uuid, run_id: Option<Uuid>) -> Self {
        Self {
            kind: kind.into(),
            run_id,
            repository_id: Some(repository_id),
            result: None,
        }
    }
}

impl Workspace {
    pub fn is_execution_only(&self) -> bool {
        self.usage == WorkspaceUsage::ExecutionOnly
    }

    pub fn require_interactive(&self) -> Result<(), WorkspaceError> {
        if self.is_execution_only() {
            return Err(WorkspaceError::ValidationError(format!(
                "Workspace {} is execution-only. Inspect its execution history and use the owning execution's controls; arbitrary development is not allowed.",
                self.id
            )));
        }
        Ok(())
    }

    /// Bind a prepared execution once, before dispatch. A later execution may
    /// not adopt the same environment by replacing its historical owner.
    pub async fn bind_execution_owner(
        pool: &SqlitePool,
        workspace_id: Uuid,
        kind: &str,
        run_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        let changed = sqlx::query(
            "UPDATE workspaces SET execution_owner=json_set(execution_owner,'$.run_id',?), updated_at=datetime('now','subsec')
             WHERE id=? AND usage='execution_only' AND json_extract(execution_owner,'$.kind')=?
               AND (json_extract(execution_owner,'$.run_id') IS NULL OR json_extract(execution_owner,'$.run_id')=?)",
        )
        .bind(run_id.to_string())
        .bind(workspace_id)
        .bind(kind)
        .bind(run_id.to_string())
        .execute(pool)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(sqlx::Error::Protocol(
                "Execution workspace owner does not match".into(),
            ));
        }
        Ok(())
    }

    pub async fn save_execution_result(
        pool: &SqlitePool,
        workspace_id: Uuid,
        kind: &str,
        run_id: Option<Uuid>,
        result: &WorkspaceExecutionResult,
    ) -> Result<(), sqlx::Error> {
        let changed = sqlx::query(
            "UPDATE workspaces SET execution_owner=json_set(execution_owner,'$.result',json(?)), updated_at=datetime('now','subsec')
             WHERE id=? AND usage='execution_only' AND json_extract(execution_owner,'$.kind')=?
               AND json_extract(execution_owner,'$.run_id') IS ?",
        )
        .bind(Json(result))
        .bind(workspace_id)
        .bind(kind)
        .bind(run_id.map(|id| id.to_string()))
        .execute(pool)
        .await?;
        if changed.rows_affected() != 1 {
            return Err(sqlx::Error::Protocol(
                "Execution result owner does not match".into(),
            ));
        }
        Ok(())
    }
}

pub async fn require_interactive(
    pool: &SqlitePool,
    workspace_id: Uuid,
) -> Result<Workspace, WorkspaceError> {
    let workspace = Workspace::find_by_id(pool, workspace_id)
        .await?
        .ok_or(WorkspaceError::WorkspaceNotFound)?;
    workspace.require_interactive()?;
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        session::{CreateSession, Session},
        workspace::{ContainerOwnership, CreateWorkspace, WorkspaceKind},
    };

    #[tokio::test]
    async fn migration_preserves_existing_workspace_sessions_and_defaults() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let full = sqlx::migrate!("./migrations");
        let mut previous = sqlx::migrate!("./migrations");
        previous.migrations = std::borrow::Cow::Owned(
            previous
                .iter()
                .filter(|m| m.version < 20260921000000)
                .cloned()
                .collect(),
        );
        previous.run(&pool).await.unwrap();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO workspaces(id,branch,name,archived,pinned,container_ref) VALUES(?,'user-branch','OpenWiki: a human name',1,1,'/unchanged')")
            .bind(id).execute(&pool).await.unwrap();
        let session = Session::create(
            &pool,
            &CreateSession {
                executor: None,
                name: Some("Preserved".into()),
            },
            Uuid::new_v4(),
            id,
        )
        .await
        .unwrap();
        full.run(&pool).await.unwrap();
        full.run(&pool).await.unwrap();
        let ws = Workspace::find_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(ws.usage, WorkspaceUsage::Interactive);
        assert!(ws.execution_owner.is_none() && ws.archived && ws.pinned);
        assert_eq!(ws.container_ref.as_deref(), Some("/unchanged"));
        assert_eq!(
            Session::find_by_id(&pool, session.id)
                .await
                .unwrap()
                .unwrap()
                .workspace_id,
            id
        );
        assert_eq!(Workspace::fetch_all(&pool).await.unwrap().len(), 1);
        assert_eq!(
            Workspace::find_all_with_status(&pool, None, None)
                .await
                .unwrap()[0]
                .usage,
            WorkspaceUsage::Interactive
        );
    }

    #[tokio::test]
    async fn usage_is_persistent_and_independent_of_kind_archive_and_owner_result() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repo = Uuid::new_v4();
        for kind in [
            OPENWIKI_BOOTSTRAP,
            OPENWIKI_SYNC,
            INTEGRATION,
            "future_product",
        ] {
            let owner = WorkspaceExecutionOwner::new(kind, repo, None);
            let ws = Workspace::create_with_owner(
                &pool,
                &CreateWorkspace {
                    branch: kind.into(),
                    name: None,
                },
                Uuid::new_v4(),
                Some(&owner),
            )
            .await
            .unwrap();
            assert!(ws.is_execution_only());
            assert!(require_interactive(&pool, ws.id).await.is_err());
            let run = Uuid::new_v4();
            Workspace::bind_execution_owner(&pool, ws.id, kind, run)
                .await
                .unwrap();
            Workspace::bind_execution_owner(&pool, ws.id, kind, run)
                .await
                .unwrap();
            assert!(
                Workspace::bind_execution_owner(&pool, ws.id, kind, Uuid::new_v4())
                    .await
                    .is_err()
            );
            assert!(
                Workspace::bind_execution_owner(&pool, ws.id, "spoofed", run)
                    .await
                    .is_err()
            );
            Workspace::save_execution_result(
                &pool,
                ws.id,
                kind,
                Some(run),
                &WorkspaceExecutionResult {
                    status: WorkspaceExecutionTerminalStatus::Succeeded,
                    completed_at: Utc::now(),
                    error: None,
                    wiki_commit: None,
                    no_op: true,
                },
            )
            .await
            .unwrap();
            Workspace::set_archived(&pool, ws.id, true).await.unwrap();
            let mut reloaded = Workspace::find_by_id_with_status(&pool, ws.id)
                .await
                .unwrap()
                .unwrap()
                .workspace;
            reloaded.workspace_kind = WorkspaceKind::DirectFolder;
            reloaded.container_ownership = ContainerOwnership::External;
            assert!(reloaded.is_execution_only() && reloaded.require_interactive().is_err());
            assert_eq!(reloaded.execution_owner.unwrap().run_id, Some(run));
        }
        let normal = Workspace::create(
            &pool,
            &CreateWorkspace {
                branch: "human".into(),
                name: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap();
        assert!(normal.require_interactive().is_ok());
        assert!(
            Workspace::bind_execution_owner(&pool, normal.id, INTEGRATION, Uuid::new_v4())
                .await
                .is_err()
        );
    }
}
