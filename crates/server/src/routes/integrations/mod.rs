//! Local Board Integration: a product of the existing Orchestration runtime.
//! The product owns source reservations, host validation and publication, not
//! another Agent scheduler. Git, Card effects and Wiki receipts are separate.
mod admission;
mod runtime;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use db::models::integration::{IntegrationRun, IntegrationSelection};
use deployment::Deployment;
use executors::profile::ExecutorConfig;
pub use runtime::spawn_monitor;
use serde::Deserialize;
use ts_rs::TS;
use utils::{
    repository_memory::{ReconciliationResult, RepositoryMemoryStore},
    response::ApiResponse,
};
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Clone, Deserialize, TS)]
pub struct CreateIntegrationRequest {
    // Stable per explicit UI submission; retries must preserve the selection.
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub repository_id: Uuid,
    pub target_branch: String,
    pub selections: Vec<IntegrationSelection>,
    pub executor_config: Option<ExecutorConfig>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    project_id: Uuid,
}

fn error(error: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(error.to_string())
}

async fn create(
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<CreateIntegrationRequest>,
) -> Result<Json<ApiResponse<IntegrationRun>>, ApiError> {
    let run = admission::submit(&deployment, request)
        .await
        .map_err(error)?;
    Ok(Json(ApiResponse::success(run)))
}

async fn list(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ApiResponse<Vec<IntegrationRun>>>, ApiError> {
    let mut runs: Vec<IntegrationRun> = sqlx::query_as(
        "SELECT * FROM integration_runs WHERE project_id=? ORDER BY created_at DESC LIMIT 100",
    )
    .bind(query.project_id)
    .fetch_all(&deployment.db().pool)
    .await?;
    for run in &mut runs {
        observe_wiki(&deployment, run).await;
    }
    Ok(Json(ApiResponse::success(runs)))
}

async fn repositories(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ApiResponse<Vec<db::models::repo::Repo>>>, ApiError> {
    let ids = project_repository_ids(&deployment.db().pool, query.project_id).await?;
    let repos = db::models::repo::Repo::find_by_ids(&deployment.db().pool, &ids).await?;
    Ok(Json(ApiResponse::success(repos)))
}

async fn project_repository_ids(
    pool: &sqlx::SqlitePool,
    project_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    // Modern local Boards link repositories through adopted Workspaces; their
    // scratch working-location default is not a project_repos membership row.
    // Use the same current association for discovery and admission. Keep legacy
    // explicit bindings, without leaking repositories from another Board.
    sqlx::query_scalar(
        "SELECT r.id FROM repos r WHERE
         EXISTS(SELECT 1 FROM project_repos p WHERE p.repo_id=r.id AND p.project_id=?)
         OR EXISTS(SELECT 1 FROM workspace_repos wr
           JOIN local_workspace_links l ON l.workspace_id=wr.workspace_id
           JOIN local_issues i ON i.id=l.issue_id
           WHERE wr.repo_id=r.id AND i.project_id=?)
         ORDER BY r.display_name,r.id",
    )
    .bind(project_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
}

async fn status(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<IntegrationRun>>, ApiError> {
    let mut run = IntegrationRun::find(&deployment.db().pool, id).await?;
    observe_wiki(&deployment, &mut run).await;
    Ok(Json(ApiResponse::success(run)))
}

/// Read-only projection of the existing outbox/receipt lifecycle. Do not keep
/// displaying the admission-time word "pending" after a successful Wiki run,
/// or equate a source Integration success with Wiki acknowledgement.
async fn observe_wiki(deployment: &DeploymentImpl, run: &mut IntegrationRun) {
    if !run.payload.published {
        return;
    }
    let result: anyhow::Result<Option<String>> = async {
        let repo = db::models::repo::Repo::find_by_id(&deployment.db().pool, run.repository_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Repository missing"))?;
        let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)?
        else {
            return Ok(None);
        };
        let Some(integration) = store
            .integrations()?
            .into_iter()
            .find(|record| record.id == run.id)
        else {
            return Ok(None);
        };
        let state = store.state()?;
        if state.target_branch.as_deref() != Some(integration.target_branch.as_str()) {
            return Ok(Some("recorded; Wiki configured for another target".into()));
        }
        let mut remaining = 0;
        for id in &integration.event_ids {
            match store.receipt(*id)? {
                Some(receipt) if receipt.result != ReconciliationResult::Failed => {}
                _ => remaining += 1,
            }
        }
        Ok(Some(if integration.event_ids.is_empty() {
            "no semantic events; no Wiki update requested".into()
        } else if remaining == 0 {
            "all selected semantic events acknowledged by OpenWiki".into()
        } else if !state.enabled {
            format!("disabled; {remaining} semantic events retained")
        } else if let Some(error) = state.error {
            format!("{remaining} semantic events pending; Wiki error: {error}")
        } else if state.last_success.is_none() {
            format!("{remaining} semantic events pending; initialise Wiki first")
        } else {
            format!(
                "{remaining} semantic events pending; Wiki status {:?}",
                state.status
            )
        }))
    }
    .await;
    match result {
        Ok(Some(status)) => run.payload.wiki_result = Some(status),
        Ok(None) => {}
        Err(error) => run.payload.wiki_result = Some(format!("Wiki status unavailable: {error:#}")),
    }
}

async fn cancel(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<IntegrationRun>>, ApiError> {
    // Publishing is past the cancellation boundary. Never attach Git undo to a
    // Card reopening or to a late duplicate cancel.
    Ok(Json(ApiResponse::success(
        IntegrationRun::request_cancel(&deployment.db().pool, id).await?,
    )))
}

async fn recover(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<IntegrationRun>>, ApiError> {
    let run = IntegrationRun::find(&deployment.db().pool, id).await?;
    if !matches!(run.status.as_str(), "post_processing" | "recovery_required") {
        return Err(error(
            "Only publication reconciliation may be retried on this run; create a new explicit selection for another integration",
        ));
    }
    runtime::request_recovery(&deployment, run)
        .await
        .map_err(error)?;
    status(State(deployment), Path(id)).await
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/integrations", get(list).post(create))
        .route("/integrations/repositories", get(repositories))
        .route("/integrations/{id}", get(status))
        .route("/integrations/{id}/cancel", post(cancel))
        .route("/integrations/{id}/recover", post(recover))
}

#[cfg(test)]
mod tests;
