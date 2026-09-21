//! Read-only execution details and the owner's existing Stop, not a second job API.
use axum::{Extension, Json, extract::State};
use db::models::{
    agent_runtime::AgentRunRecord,
    integration::IntegrationRun,
    repo::Repo,
    workspace::Workspace,
    workspace_usage::{INTEGRATION, OPENWIKI_BOOTSTRAP, OPENWIKI_SYNC, WorkspaceExecutionOwner},
};
use deployment::Deployment;
use executors::runtime::{
    AgentRunPortCommand, AgentRunPortCommandEnvelope, AgentRunStatus,
    ORCHESTRATION_COMMAND_SCHEMA_VERSION,
};
use serde::Serialize;
use services::services::agent_runtime::AgentRunCommandService;
use ts_rs::TS;
use utils::{repository_memory::RepositoryMemoryStore, response::ApiResponse};
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Clone, Serialize, TS)]
pub struct WorkspaceExecutionView {
    pub workspace_id: Uuid,
    pub owner: Option<WorkspaceExecutionOwner>,
    pub status: String,
    pub terminal: bool,
    pub can_stop: bool,
    pub stop_requested: bool,
    pub error: Option<String>,
    pub files_available: bool,
    pub published: bool,
}

fn error(message: impl std::fmt::Display) -> ApiError {
    ApiError::Conflict(message.to_string())
}

type BootstrapIdentityAndOutcome = (String, Option<String>, Option<Uuid>, Option<Uuid>);

pub async fn describe(
    deployment: &DeploymentImpl,
    workspace: &Workspace,
) -> Result<WorkspaceExecutionView, ApiError> {
    let pool = &deployment.db().pool;
    let mut view = WorkspaceExecutionView {
        workspace_id: workspace.id,
        owner: workspace
            .execution_owner
            .as_ref()
            .map(|owner| owner.0.clone()),
        status: "unknown".into(),
        terminal: false,
        can_stop: false,
        stop_requested: false,
        error: None,
        files_available: services::services::workspace_usage::inspection_root(workspace).is_ok(),
        published: false,
    };
    let Some(owner) = &workspace.execution_owner else {
        view.error = Some("Execution ownership is unavailable; inspection only".into());
        return Ok(view);
    };
    if let Some(result) = &owner.result {
        view.status = serde_json::to_value(result.status)
            .map_err(error)?
            .as_str()
            .unwrap_or("unknown")
            .into();
        view.terminal = true;
        view.error = result.error.clone();
        view.published = result.wiki_commit.is_some();
    }
    match (owner.kind.as_str(), owner.run_id) {
        (INTEGRATION, Some(id)) => {
            let run = IntegrationRun::find(pool, id).await?;
            if run.workspace_id != Some(workspace.id)
                || Some(run.repository_id) != owner.repository_id
            {
                return Err(error("Integration workspace ownership mismatch"));
            }
            view.status = run.status;
            view.terminal = matches!(
                view.status.as_str(),
                "succeeded" | "failed" | "cancelled" | "blocked"
            );
            view.stop_requested = run.cancel_requested;
            view.can_stop = !run.cancel_requested
                && matches!(
                    view.status.as_str(),
                    "queued" | "preparing" | "integrating" | "validating"
                );
            view.published = run.payload.published;
            view.error = run.error;
        }
        (OPENWIKI_BOOTSTRAP, Some(id)) => {
            let row: Option<BootstrapIdentityAndOutcome> = sqlx::query_as("SELECT status,error_text,workspace_id,repository_id FROM workflow_runs WHERE id=? AND issue_id IS NULL AND trigger_source='openwiki_bootstrap'")
                .bind(id).fetch_optional(pool).await?;
            if let Some((status, reason, ws, repo)) = row {
                if ws != Some(workspace.id) || repo != owner.repository_id {
                    return Err(error("Bootstrap workspace ownership mismatch"));
                }
                view.status = status;
                view.error = reason;
                view.terminal = matches!(view.status.as_str(), "succeeded" | "failed" | "canceled");
                view.can_stop = !view.terminal;
            }
            if let Some(store) = owner_store(deployment, owner).await? {
                let state = store.state()?;
                if state.maintenance_workspace_id == Some(workspace.id)
                    && state
                        .bootstrap
                        .as_ref()
                        .is_some_and(|active| active.workflow_run_id == id)
                {
                    let phase = state.bootstrap.as_ref().unwrap().phase;
                    view.stop_requested =
                        phase == utils::repository_memory::OpenWikiBootstrapPhase::CleaningUp;
                    view.can_stop &= !view.stop_requested && store.publication(id)?.is_none();
                    if !view.terminal {
                        view.status = format!("{phase:?}").to_lowercase();
                    }
                    if state.error.is_some() {
                        view.error = state.error;
                    }
                }
            }
            // Workflow success is after publication, not Generator success.
            view.published = view.terminal && view.status == "succeeded";
        }
        (OPENWIKI_SYNC, Some(id)) => {
            if let Some(store) = owner_store(deployment, owner).await? {
                let state = store.state()?;
                if state.maintenance_workspace_id == Some(workspace.id)
                    && state.active_run_id == Some(id)
                {
                    let run = AgentRunRecord::find(pool, id)
                        .await?
                        .ok_or_else(|| error("Maintenance AgentRun missing"))?;
                    if run.workspace_id != workspace.id
                        || state.maintenance_session_id != Some(run.session_id)
                    {
                        return Err(error("Sync workspace ownership mismatch"));
                    }
                    view.terminal = false;
                    view.stop_requested = run.status == AgentRunStatus::Cancelling;
                    view.can_stop = !run.status.is_terminal()
                        && !view.stop_requested
                        && store.publication(id)?.is_none();
                    view.status = if run.status.is_terminal() {
                        "finalizing".into()
                    } else {
                        serde_json::to_value(run.status)
                            .map_err(error)?
                            .as_str()
                            .unwrap_or("unknown")
                            .into()
                    };
                    view.error = state.error;
                }
            }
        }
        (_, None) if owner.result.is_none() => {
            view.error = Some("Owner preparation incomplete or historical result unknown".into());
        }
        _ => {
            view.error = Some("Unrecognized execution owner; inspection only".into());
        }
    }
    Ok(view)
}

async fn owner_store(
    deployment: &DeploymentImpl,
    owner: &WorkspaceExecutionOwner,
) -> Result<Option<RepositoryMemoryStore>, ApiError> {
    let Some(id) = owner.repository_id else {
        return Ok(None);
    };
    let Some(repo) = Repo::find_by_id(&deployment.db().pool, id).await? else {
        return Ok(None);
    };
    Ok(RepositoryMemoryStore::existing_for_repository(
        &repo.name, id,
    )?)
}

pub async fn get(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<WorkspaceExecutionView>>, ApiError> {
    Ok(Json(ApiResponse::success(
        describe(&deployment, &workspace).await?,
    )))
}

pub async fn list(
    State(deployment): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<Vec<WorkspaceExecutionView>>>, ApiError> {
    let mut views = Vec::new();
    for ws in Workspace::find_all_with_status(&deployment.db().pool, None, None).await? {
        if !ws.workspace.is_execution_only() {
            continue;
        }
        let view = match describe(&deployment, &ws.workspace).await {
            Ok(view) => view,
            Err(error) => WorkspaceExecutionView {
                workspace_id: ws.workspace.id,
                owner: ws.workspace.execution_owner.map(|owner| owner.0),
                status: "unknown".into(),
                terminal: false,
                can_stop: false,
                stop_requested: false,
                error: Some(error.to_string()),
                files_available: false,
                published: false,
            },
        };
        views.push(view);
    }
    Ok(Json(ApiResponse::success(views)))
}

pub async fn stop(deployment: &DeploymentImpl, workspace: &Workspace) -> Result<(), ApiError> {
    let view = describe(deployment, workspace).await?;
    if view.terminal || view.stop_requested {
        return Ok(());
    }
    if !view.can_stop {
        return Err(error(
            "Owner cannot stop in this phase; inspect its result or recovery state. Published Git changes are not undone.",
        ));
    }
    let owner = workspace
        .execution_owner
        .as_ref()
        .ok_or_else(|| error("Owner missing"))?;
    let id = owner.run_id.ok_or_else(|| error("Owner is unbound"))?;
    match owner.kind.as_str() {
        INTEGRATION => {
            IntegrationRun::request_cancel(&deployment.db().pool, id).await?;
        }
        OPENWIKI_BOOTSTRAP => {
            crate::workflow_runtime::bootstrap::cancel_owned_run(deployment, id).await?;
        }
        OPENWIKI_SYNC => {
            let store = owner_store(deployment, owner)
                .await?
                .ok_or_else(|| error("Maintenance owner missing"))?;
            let _lock = store.try_lock().map_err(error)?;
            let state = store.state()?;
            if state.active_run_id != Some(id)
                || state.maintenance_workspace_id != Some(workspace.id)
                || state.bootstrap.is_some()
            {
                return Err(error("Sync ownership changed; stop rejected"));
            }
            if store.publication(id)?.is_some() {
                return Err(error(
                    "Wiki publication already started; its outcome must be reconciled, not undone",
                ));
            }
            let run = AgentRunRecord::find(&deployment.db().pool, id)
                .await?
                .ok_or_else(|| error("Maintenance AgentRun missing"))?;
            if run.status.is_terminal() {
                return Ok(());
            }
            AgentRunCommandService::new(&deployment.db().pool, deployment.agent_run_port())
                .dispatch(AgentRunPortCommandEnvelope {
                    schema_version: ORCHESTRATION_COMMAND_SCHEMA_VERSION,
                    command_id: crate::workflow_runtime::runner::stable_workflow_identity(
                        id,
                        id,
                        0,
                        "workspace-owner-stop",
                    ),
                    idempotency_key: format!("workspace-owner-stop:{id}"),
                    agent_run_id: id,
                    orchestration_run_id: None,
                    orchestration_node_execution_id: None,
                    correlation_id: run.correlation_id,
                    created_at: run.created_at,
                    command: AgentRunPortCommand::Cancel {
                        reason: "OpenWiki Sync cancelled by the user".into(),
                    },
                })
                .await
                .map_err(error)?;
        }
        _ => {
            return Err(error(
                "Unknown execution owner; stop must not bypass audit/ownership",
            ));
        }
    }
    Ok(())
}
