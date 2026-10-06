//! Resource ownership is deterministic; the Codex mediator only proposes.
mod runtime;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use db::models::resource_coordination::{
    self as model, RegisterResource, ResourceEvent, ResourceMediation, ResourceOperation,
    ResourceOperationSpec, ResourceRecovery, ResourceSnapshot, SharedResource,
};
use deployment::Deployment;
pub use runtime::spawn_monitor;
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

fn error(e: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(e.to_string())
}

async fn snapshot(
    State(d): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<ResourceSnapshot>>, ApiError> {
    Ok(Json(ApiResponse::success(
        model::snapshot(&d.db().pool).await.map_err(error)?,
    )))
}
async fn register(
    State(d): State<DeploymentImpl>,
    Json(r): Json<RegisterResource>,
) -> Result<Json<ApiResponse<SharedResource>>, ApiError> {
    Ok(Json(ApiResponse::success(
        SharedResource::register(&d.db().pool, r)
            .await
            .map_err(error)?,
    )))
}
async fn submit(
    State(d): State<DeploymentImpl>,
    Json(r): Json<ResourceOperationSpec>,
) -> Result<Json<ApiResponse<ResourceOperation>>, ApiError> {
    let _admission = services::services::integration_admission::MUTATIONS
        .lock()
        .await;
    Ok(Json(ApiResponse::success(
        ResourceOperation::submit(&d.db().pool, r)
            .await
            .map_err(error)?,
    )))
}
#[derive(Deserialize, Default)]
struct OperationQuery {
    workspace_id: Option<Uuid>,
    #[serde(default)]
    wait_seconds: u64,
}
async fn list(
    State(d): State<DeploymentImpl>,
    Query(q): Query<OperationQuery>,
) -> Result<Json<ApiResponse<Vec<ResourceOperation>>>, ApiError> {
    let rows = sqlx::query_as("SELECT * FROM resource_operations WHERE (? IS NULL OR workspace_id=?) ORDER BY sequence DESC LIMIT 100")
        .bind(q.workspace_id).bind(q.workspace_id).fetch_all(&d.db().pool).await?;
    Ok(Json(ApiResponse::success(rows)))
}
async fn operation(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Query(q): Query<OperationQuery>,
) -> Result<Json<ApiResponse<ResourceOperation>>, ApiError> {
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(q.wait_seconds.min(25));
    loop {
        let op = ResourceOperation::find(&d.db().pool, id)
            .await
            .map_err(error)?;
        if !matches!(op.status.as_str(), "queued" | "launching" | "running")
            || tokio::time::Instant::now() >= deadline
        {
            return Ok(Json(ApiResponse::success(op)));
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}
async fn cancel(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<ResourceOperation>>, ApiError> {
    ResourceOperation::cancel(&d.db().pool, id)
        .await
        .map_err(error)?;
    let op = ResourceOperation::find(&d.db().pool, id)
        .await
        .map_err(error)?;
    if op.status == "recovery_required" {
        runtime::stop_operation_commands(&d, &op)
            .await
            .map_err(error)?;
    }
    Ok(Json(ApiResponse::success(
        ResourceOperation::find(&d.db().pool, id)
            .await
            .map_err(error)?,
    )))
}
async fn recover(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Json(r): Json<ResourceRecovery>,
) -> Result<Json<ApiResponse<ResourceOperation>>, ApiError> {
    // No force-unlock API: inspect all matching processes, including a launch
    // interrupted before process_id was saved. Remote effects need user evidence.
    let op = ResourceOperation::find(&d.db().pool, id)
        .await
        .map_err(error)?;
    runtime::require_processes_settled(&d, &op)
        .await
        .map_err(error)?;
    ResourceOperation::recover(&d.db().pool, id, r)
        .await
        .map_err(error)?;
    Ok(Json(ApiResponse::success(
        ResourceOperation::find(&d.db().pool, id)
            .await
            .map_err(error)?,
    )))
}
async fn mediate(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Option<ResourceMediation>>>, ApiError> {
    Ok(Json(ApiResponse::success(
        ResourceMediation::request(&d.db().pool, id, true)
            .await
            .map_err(error)?,
    )))
}
async fn mediations(
    State(d): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<Vec<ResourceMediation>>>, ApiError> {
    Ok(Json(ApiResponse::success(
        sqlx::query_as("SELECT * FROM resource_mediations ORDER BY created_at DESC LIMIT 30")
            .fetch_all(&d.db().pool)
            .await?,
    )))
}
async fn stop_mediation(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    cancel_mediation(&d, id).await.map_err(error)?;
    Ok(Json(ApiResponse::success(())))
}
pub(crate) async fn cancel_mediation(d: &DeploymentImpl, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("UPDATE resource_mediations SET status='failed',result='Mediation cancelled by the user; resource ownership unchanged' WHERE id=? AND status IN ('pending','preparing','running')").bind(id).execute(&d.db().pool).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM orchestration_runs WHERE id=?)")
            .bind(id)
            .fetch_one(&d.db().pool)
            .await?;
    if exists {
        services::services::orchestration::OrchestrationService::new(
            d.db().pool.clone(),
            std::sync::Arc::new(d.agent_run_port().clone()),
        )
        .cancel(id, id)
        .await?;
    }
    Ok(())
}
#[derive(Deserialize, Default)]
struct EventQuery {
    #[serde(default)]
    after: i64,
}
async fn events(
    State(d): State<DeploymentImpl>,
    Query(q): Query<EventQuery>,
) -> Result<Json<ApiResponse<Vec<ResourceEvent>>>, ApiError> {
    Ok(Json(ApiResponse::success(
        sqlx::query_as(
            "SELECT * FROM resource_events WHERE sequence>? ORDER BY sequence LIMIT 200",
        )
        .bind(q.after)
        .fetch_all(&d.db().pool)
        .await?,
    )))
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new().nest(
        "/resource-coordination",
        Router::new()
            .route("/snapshot", get(snapshot))
            .route("/resources", post(register))
            .route("/operations", get(list).post(submit))
            .route("/operations/{id}", get(operation))
            .route("/operations/{id}/cancel", post(cancel))
            .route("/operations/{id}/recover", post(recover))
            .route("/operations/{id}/mediate", post(mediate))
            .route("/mediations", get(mediations))
            .route("/mediations/{id}/cancel", post(stop_mediation))
            .route("/events", get(events)),
    )
}
