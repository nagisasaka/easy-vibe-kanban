use std::path::Path;

use axum::{
    Json, Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::{get, post},
};
use executors::agent_tools::{
    AgentToolDefinition, AgentToolLocator, AgentToolOperationError, AgentToolService,
    CopyAgentToolRequest, RemoveAgentToolRequest, ToggleAgentToolRequest,
    public_api::{
        AgentToolInventoryView, AgentToolView, CopyAgentToolView, CreateAgentToolWriteRequest,
        ReadAgentToolDefinitionRequest, UpdateAgentToolWriteRequest, public_tool_error,
    },
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utils::response::ApiResponse;

use crate::DeploymentImpl;

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/agent-tools", get(discover).post(create).put(update))
        .route("/agent-tools/remove", post(remove))
        .route("/agent-tools/toggle", post(toggle))
        .route("/agent-tools/copy", post(copy))
        .route("/agent-tools/reveal", post(reveal))
        .route("/agent-tools/read-definition", post(read_definition))
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct AgentToolDiscoveryQuery {
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct AgentToolRevealResponse {
    pub native_path: String,
}

fn service() -> Result<AgentToolService, AgentToolOperationError> {
    AgentToolService::from_system().map_err(|error| public_tool_error(error, None, None))
}

fn project_path(value: Option<&str>) -> Option<&Path> {
    value.map(Path::new)
}

fn operation_error(
    error: executors::agent_tools::AgentToolError,
    locator: &AgentToolLocator,
) -> AgentToolOperationError {
    public_tool_error(error, Some(locator.provider), Some(locator.name.clone()))
}

async fn discover(
    State(_deployment): State<DeploymentImpl>,
    Query(query): Query<AgentToolDiscoveryQuery>,
) -> ResponseJson<ApiResponse<AgentToolInventoryView, AgentToolOperationError>> {
    match service() {
        Ok(service) => ResponseJson(ApiResponse::success(
            service
                .discover(project_path(query.project_path.as_deref()))
                .into(),
        )),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn create(
    State(_deployment): State<DeploymentImpl>,
    Json(request): Json<CreateAgentToolWriteRequest>,
) -> ResponseJson<ApiResponse<AgentToolView, AgentToolOperationError>> {
    let locator = request.target.clone();
    match service().and_then(|service| {
        service
            .create_from_write(request)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(item) => ResponseJson(ApiResponse::success(item)),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn update(
    State(_deployment): State<DeploymentImpl>,
    Json(request): Json<UpdateAgentToolWriteRequest>,
) -> ResponseJson<ApiResponse<AgentToolView, AgentToolOperationError>> {
    let locator = request.target.clone();
    match service().and_then(|service| {
        service
            .update_from_write(request)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(item) => ResponseJson(ApiResponse::success(item)),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn remove(
    State(_deployment): State<DeploymentImpl>,
    Json(request): Json<RemoveAgentToolRequest>,
) -> ResponseJson<ApiResponse<(), AgentToolOperationError>> {
    let locator = request.target.clone();
    match service().and_then(|service| {
        service
            .remove(request)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(()) => ResponseJson(ApiResponse::success(())),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn toggle(
    State(_deployment): State<DeploymentImpl>,
    Json(request): Json<ToggleAgentToolRequest>,
) -> ResponseJson<ApiResponse<AgentToolView, AgentToolOperationError>> {
    let locator = request.target.clone();
    match service().and_then(|service| {
        service
            .set_enabled(request)
            .map(Into::into)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(item) => ResponseJson(ApiResponse::success(item)),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn copy(
    State(_deployment): State<DeploymentImpl>,
    Json(request): Json<CopyAgentToolRequest>,
) -> ResponseJson<ApiResponse<CopyAgentToolView, AgentToolOperationError>> {
    let locator = request.source.clone();
    match service().and_then(|service| {
        service
            .copy(request)
            .map(Into::into)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(item) => ResponseJson(ApiResponse::success(item)),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn reveal(
    State(_deployment): State<DeploymentImpl>,
    Json(locator): Json<AgentToolLocator>,
) -> ResponseJson<ApiResponse<AgentToolRevealResponse, AgentToolOperationError>> {
    match service().and_then(|service| {
        service
            .get(&locator)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(item) => ResponseJson(ApiResponse::success(AgentToolRevealResponse {
            native_path: item.native_path,
        })),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}

async fn read_definition(
    State(_deployment): State<DeploymentImpl>,
    Json(request): Json<ReadAgentToolDefinitionRequest>,
) -> ResponseJson<ApiResponse<AgentToolDefinition, AgentToolOperationError>> {
    let locator = request.target.clone();
    match service().and_then(|service| {
        service
            .read_definition(request)
            .map_err(|error| operation_error(error, &locator))
    }) {
        Ok(definition) => ResponseJson(ApiResponse::success(definition)),
        Err(error) => ResponseJson(ApiResponse::error_with_data(error)),
    }
}
