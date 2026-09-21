use std::path::Path;

use axum::{
    Json, Router,
    extract::Query,
    routing::{get, post},
};
use executors::agent_settings::{
    AgentSettingError, AgentSettingOperationError, AgentSettingsProvider, AgentSettingsService,
    ApplyNativeFileRequest, ApplySettingsRequest, NativeConfigFile, SettingsDiff, SettingsPatch,
    public_api::{
        AgentSettingsInventoryView, ConfigProfileView, ConfirmProfileApplyRequest,
        CopyProfileWriteRequest, NativeSettingsEditRequest, ProfileApplyRequest,
        ProfileCopyPreviewView, ReadNativeSettingsRequest, SettingsSnapshotView,
        WriteConfigProfileRequest, public_diff, public_error,
    },
};
use serde::Deserialize;
use ts_rs::TS;
use utils::response::ApiResponse;

use crate::DeploymentImpl;

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/agent-settings", get(discover))
        .route("/agent-settings/diff", post(diff))
        .route("/agent-settings/apply", post(apply))
        .route("/agent-settings/native-file/read", post(read_native_file))
        .route("/agent-settings/native-file/diff", post(diff_native_file))
        .route("/agent-settings/native-file/apply", post(apply_native_file))
        .route(
            "/agent-settings/profiles",
            get(list_profiles).post(write_profile),
        )
        .route(
            "/agent-settings/profiles/copy-preview",
            post(copy_profile_preview),
        )
        .route(
            "/agent-settings/profiles/apply-preview",
            post(profile_apply_preview),
        )
        .route("/agent-settings/profiles/apply", post(apply_profile))
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct AgentSettingsDiscoveryQuery {
    #[serde(default)]
    pub provider: Option<AgentSettingsProvider>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct AgentSettingsProfilesQuery {
    #[serde(default)]
    pub provider: Option<AgentSettingsProvider>,
}

type Response<T> = Json<ApiResponse<T, AgentSettingOperationError>>;

fn respond<T>(
    result: Result<T, AgentSettingError>,
    provider: Option<AgentSettingsProvider>,
) -> Response<T> {
    Json(match result {
        Ok(value) => ApiResponse::success(value),
        Err(error) => ApiResponse::error_with_data(public_error(error, provider)),
    })
}

async fn discover(
    Query(query): Query<AgentSettingsDiscoveryQuery>,
) -> Response<AgentSettingsInventoryView> {
    respond(
        AgentSettingsService::from_system().map(|service| {
            service
                .discover(query.provider, query.project_path.as_deref().map(Path::new))
                .into()
        }),
        query.provider,
    )
}

async fn diff(Json(patch): Json<SettingsPatch>) -> Response<SettingsDiff> {
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.diff(&patch))
            .map(public_diff),
        Some(patch.provider),
    )
}

async fn apply(Json(request): Json<ApplySettingsRequest>) -> Response<SettingsSnapshotView> {
    let provider = request.patch.provider;
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.apply(request))
            .map(Into::into),
        Some(provider),
    )
}

async fn read_native_file(
    Json(request): Json<ReadNativeSettingsRequest>,
) -> Response<NativeConfigFile> {
    let provider = request.provider;
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.read_native_settings(request)),
        Some(provider),
    )
}

async fn diff_native_file(
    Json(request): Json<NativeSettingsEditRequest>,
) -> Response<SettingsDiff> {
    let provider = request.patch.provider;
    respond(
        AgentSettingsService::from_system().and_then(|service| {
            if !request.confirmed_sensitive_read {
                return Err(AgentSettingError::InvalidRequest(
                    "explicit sensitive read required".into(),
                ));
            }
            service.diff_native_file(&request.patch)
        }),
        Some(provider),
    )
}

async fn apply_native_file(
    Json(request): Json<ApplyNativeFileRequest>,
) -> Response<SettingsSnapshotView> {
    let provider = request.patch.provider;
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.apply_native_file(request))
            .map(Into::into),
        Some(provider),
    )
}

async fn list_profiles(
    Query(query): Query<AgentSettingsProfilesQuery>,
) -> Response<Vec<ConfigProfileView>> {
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.list_profiles(query.provider))
            .map(|profiles| profiles.into_iter().map(Into::into).collect()),
        query.provider,
    )
}

async fn write_profile(
    Json(request): Json<WriteConfigProfileRequest>,
) -> Response<Option<ConfigProfileView>> {
    respond(
        AgentSettingsService::from_system().and_then(|service| service.write_profile(request)),
        None,
    )
}

async fn copy_profile_preview(
    Json(request): Json<CopyProfileWriteRequest>,
) -> Response<ProfileCopyPreviewView> {
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.preview_profile_copy_view(request)),
        None,
    )
}

async fn profile_apply_preview(Json(request): Json<ProfileApplyRequest>) -> Response<SettingsDiff> {
    respond(
        AgentSettingsService::from_system()
            .and_then(|service| service.preview_profile_apply_view(&request)),
        None,
    )
}

async fn apply_profile(
    Json(request): Json<ConfirmProfileApplyRequest>,
) -> Response<SettingsSnapshotView> {
    respond(
        AgentSettingsService::from_system().and_then(|service| service.apply_profile_view(request)),
        None,
    )
}
