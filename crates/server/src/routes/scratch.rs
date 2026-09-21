use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State, ws::Message},
    response::{IntoResponse, Json as ResponseJson},
    routing::get,
};
use db::models::scratch::{CreateScratch, DeleteScratch, Scratch, ScratchType, UpdateScratch};
use deployment::Deployment;
use futures_util::{StreamExt, TryStreamExt};
use serde::Deserialize;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{
    DeploymentImpl,
    error::ApiError,
    middleware::signed_ws::{MaybeSignedWebSocket, SignedWsUpgrade},
};

/// Path parameters for scratch routes with composite key
#[derive(Deserialize)]
pub struct ScratchPath {
    scratch_type: ScratchType,
    id: Uuid,
}

pub async fn list_scratch(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<Scratch>>>, ApiError> {
    let scratch_items = Scratch::find_all(&deployment.db().pool).await?;
    Ok(ResponseJson(ApiResponse::success(scratch_items)))
}

pub async fn get_scratch(
    State(deployment): State<DeploymentImpl>,
    Path(ScratchPath { scratch_type, id }): Path<ScratchPath>,
) -> Result<ResponseJson<ApiResponse<Scratch>>, ApiError> {
    let scratch = Scratch::find_by_id(&deployment.db().pool, id, &scratch_type)
        .await?
        .ok_or_else(|| ApiError::BadRequest("Scratch not found".to_string()))?;
    Ok(ResponseJson(ApiResponse::success(scratch)))
}

pub async fn create_scratch(
    State(deployment): State<DeploymentImpl>,
    Path(ScratchPath { scratch_type, id }): Path<ScratchPath>,
    Json(payload): Json<CreateScratch>,
) -> Result<ResponseJson<ApiResponse<Scratch>>, ApiError> {
    // Reject edits to draft_follow_up if a message is queued for this workspace
    if matches!(scratch_type, ScratchType::DraftFollowUp)
        && deployment.queued_message_service().has_queued(id)
    {
        return Err(ApiError::BadRequest(
            "Cannot edit scratch while a message is queued".to_string(),
        ));
    }

    // Validate that payload type matches URL type
    payload
        .payload
        .validate_type(scratch_type)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let scratch = Scratch::create(&deployment.db().pool, id, &payload).await?;
    Ok(ResponseJson(ApiResponse::success(scratch)))
}

pub async fn update_scratch(
    State(deployment): State<DeploymentImpl>,
    Path(ScratchPath { scratch_type, id }): Path<ScratchPath>,
    Json(payload): Json<UpdateScratch>,
) -> Result<ResponseJson<ApiResponse<Scratch>>, ApiError> {
    // Reject edits to draft_follow_up if a message is queued for this workspace
    if matches!(scratch_type, ScratchType::DraftFollowUp)
        && deployment.queued_message_service().has_queued(id)
    {
        return Err(ApiError::BadRequest(
            "Cannot edit scratch while a message is queued".to_string(),
        ));
    }

    // Validate that payload type matches URL type
    payload
        .payload
        .validate_type(scratch_type)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Upsert: creates if not exists, updates if exists
    let scratch = Scratch::update(&deployment.db().pool, id, &scratch_type, &payload).await?;
    Ok(ResponseJson(ApiResponse::success(scratch)))
}

pub async fn delete_scratch(
    State(deployment): State<DeploymentImpl>,
    Path(ScratchPath { scratch_type, id }): Path<ScratchPath>,
    body: Bytes,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let condition = parse_delete_condition(&body)?;
    if let Some(expected) = condition.expected_payload {
        expected
            .validate_type(scratch_type)
            .map_err(|error| ApiError::BadRequest(error.to_string()))?;
        Scratch::delete_if_unchanged(&deployment.db().pool, id, &scratch_type, &expected).await?;
        // Already absent and a newer draft are both safe acknowledgement no-ops.
        return Ok(ResponseJson(ApiResponse::success(())));
    }
    let rows = Scratch::delete(&deployment.db().pool, id, &scratch_type).await?;
    if rows == 0 {
        return Err(ApiError::BadRequest("Scratch not found".to_string()));
    }
    Ok(ResponseJson(ApiResponse::success(())))
}

fn parse_delete_condition(body: &[u8]) -> Result<DeleteScratch, ApiError> {
    // Legacy clients send application/json with an empty DELETE body.
    if body.is_empty() {
        return Ok(DeleteScratch::default());
    }
    serde_json::from_slice(body)
        .map_err(|error| ApiError::BadRequest(format!("Invalid draft acknowledgement: {error}")))
}

pub async fn stream_scratch_ws(
    ws: SignedWsUpgrade,
    State(deployment): State<DeploymentImpl>,
    Path(ScratchPath { scratch_type, id }): Path<ScratchPath>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_scratch_ws(socket, deployment, id, scratch_type).await {
            tracing::warn!("scratch WS closed: {}", e);
        }
    })
}

async fn handle_scratch_ws(
    mut socket: MaybeSignedWebSocket,
    deployment: DeploymentImpl,
    id: Uuid,
    scratch_type: ScratchType,
) -> anyhow::Result<()> {
    let mut stream = deployment
        .events()
        .stream_scratch_raw(id, &scratch_type)
        .await?
        .map_ok(|msg| msg.to_ws_message_unchecked());

    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(msg)) => {
                        if socket.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("scratch stream error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            inbound = socket.recv() => {
                match inbound {
                    Ok(Some(Message::Close(_))) => break,
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    }
    Ok(())
}

pub fn router(_deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    Router::new()
        .route("/scratch", get(list_scratch))
        .route(
            "/scratch/{scratch_type}/{id}",
            get(get_scratch)
                .post(create_scratch)
                .put(update_scratch)
                .delete(delete_scratch),
        )
        .route(
            "/scratch/{scratch_type}/{id}/stream/ws",
            get(stream_scratch_ws),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_empty_delete_and_conditional_acknowledgements_are_distinct() {
        assert!(
            super::parse_delete_condition(b"")
                .unwrap()
                .expected_payload
                .is_none()
        );
        assert!(
            super::parse_delete_condition(br#"{}"#)
                .unwrap()
                .expected_payload
                .is_none()
        );
        assert!(super::parse_delete_condition(br#"{"expected_payload": false}"#).is_err());
        assert!(super::parse_delete_condition(br#"{"expected_payload":{"type":"DRAFT_FOLLOW_UP","data":{"message":"a","executor_config":{"executor":"CODEX"}}}}"#).unwrap().expected_payload.is_some());
    }
}
