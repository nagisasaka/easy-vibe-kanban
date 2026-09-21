use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use db::models::{
    execution_process::ExecutionProcess, session::Session, tag::Tag, workspace::Workspace,
};
use deployment::Deployment;
use uuid::Uuid;

use crate::DeploymentImpl;

pub async fn load_workspace_middleware(
    State(deployment): State<DeploymentImpl>,
    Path(workspace_id): Path<Uuid>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let mutating = !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    );
    let owner_stop = request.uri().path().ends_with("/execution/stop");
    let _admission = if mutating && !owner_stop {
        Some(
            services::services::integration_admission::MUTATIONS
                .lock()
                .await,
        )
    } else {
        None
    };
    load_workspace_from_pool(
        State(deployment.db().pool.clone()),
        Path(workspace_id),
        request,
        next,
    )
    .await
}

async fn load_workspace_from_pool(
    State(pool): State<sqlx::SqlitePool>,
    Path(workspace_id): Path<Uuid>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let owner_stop = request.uri().path().ends_with("/execution/stop");
    // Shared with route-level tests: the handler is never entered on rejection.
    let workspace = match Workspace::find_by_id(&pool, workspace_id).await {
        Ok(Some(w)) => w,
        Ok(None) => {
            tracing::warn!("Workspace {} not found", workspace_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Failed to fetch Workspace {}: {}", workspace_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Although this endpoint uses GET, the relay consumes it as authorisation
    // to create an SSH tunnel and launch an editor. File inspection uses its
    // own read-only endpoints and must not grant this development capability.
    if workspace.is_execution_only() && request.uri().path().ends_with("/integration/editor/path") {
        use axum::response::IntoResponse;
        return Ok(crate::error::ApiError::Conflict(
            "Execution-only workspace cannot be opened for development in an editor".into(),
        )
        .into_response());
    }

    // Read/log operations remain available. All Workspace mutations (Git,
    // branch, scripts, editor setup, files, lifecycle) share the reservation
    // gate, rather than relying on disabled buttons or an incomplete route list.
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) && !request.uri().path().ends_with("/seen")
    {
        if workspace.is_execution_only() && !owner_stop {
            use axum::response::IntoResponse;
            return Ok(crate::error::ApiError::Conflict(format!(
                "Workspace {} is execution-only; inspect /api/workspaces/{}/usage and use its owner controls", workspace.id, workspace.id
            )).into_response());
        }
        if !(workspace.is_execution_only() && owner_stop) {
            match db::models::integration::workspace_owner(&pool, workspace_id).await {
                Ok(Some(run)) => {
                    use axum::response::IntoResponse;
                    return Ok(crate::error::ApiError::Conflict(format!(
                    "Workspace reserved by Integration {run}; use its Board progress/Cancel action"
                ))
                .into_response());
                }
                Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
                Ok(None) => {}
            }
        }
    }
    // Insert the workspace into extensions
    request.extensions_mut().insert(workspace);

    // Continue on
    Ok(next.run(request).await)
}

pub async fn load_execution_process_middleware(
    State(deployment): State<DeploymentImpl>,
    Path(process_id): Path<Uuid>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Load the execution process from the database
    let execution_process =
        match ExecutionProcess::find_by_id(&deployment.db().pool, process_id).await {
            Ok(Some(process)) => process,
            Ok(None) => {
                tracing::warn!("ExecutionProcess {} not found", process_id);
                return Err(StatusCode::NOT_FOUND);
            }
            Err(e) => {
                tracing::error!("Failed to fetch execution process {}: {}", process_id, e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

    // Inject the execution process into the request
    request.extensions_mut().insert(execution_process);

    // Continue to the next middleware/handler
    Ok(next.run(request).await)
}

// Middleware that loads and injects Tag based on the tag_id path parameter
pub async fn load_tag_middleware(
    State(deployment): State<DeploymentImpl>,
    Path(tag_id): Path<Uuid>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Load the tag from the database
    let tag = match Tag::find_by_id(&deployment.db().pool, tag_id).await {
        Ok(Some(tag)) => tag,
        Ok(None) => {
            tracing::warn!("Tag {} not found", tag_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Failed to fetch tag {}: {}", tag_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Insert the tag as an extension
    let mut request = request;
    request.extensions_mut().insert(tag);

    // Continue with the next middleware/handler
    Ok(next.run(request).await)
}

pub async fn load_session_middleware(
    State(deployment): State<DeploymentImpl>,
    Path(session_id): Path<Uuid>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    load_session_from_pool(
        State(deployment.db().pool.clone()),
        Path(session_id),
        request,
        next,
    )
    .await
}

async fn load_session_from_pool(
    State(pool): State<sqlx::SqlitePool>,
    Path(session_id): Path<Uuid>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let session = match Session::find_by_id(&pool, session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            tracing::warn!("Session {} not found", session_id);
            return Err(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            tracing::error!("Failed to fetch session {}: {}", session_id, e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) && let Err(error) =
        db::models::workspace_usage::require_interactive(&pool, session.workspace_id).await
    {
        use axum::response::IntoResponse;
        return Ok(crate::error::ApiError::Conflict(error.to_string()).into_response());
    }
    request.extensions_mut().insert(session);
    Ok(next.run(request).await)
}

#[cfg(test)]
#[path = "model_loaders_tests.rs"]
mod usage_tests;
