use std::path::PathBuf;

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::{get, put},
};
use db::models::{workspace::Workspace, workspace_repo::WorkspaceRepo};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use services::services::{
    container::ContainerService,
    wiki::{self, WikiError, WikiSnapshot},
};
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use super::files::resolve_workspace_repo_root;
use crate::{DeploymentImpl, error::ApiError};

const MAX_SOURCE_LINKS: usize = 500;

#[derive(Debug, Deserialize)]
pub struct WikiRepoQuery {
    pub repo_id: Uuid,
    #[serde(default)]
    pub openwiki: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct WorkspaceWikiSnapshot {
    pub workspace_id: Uuid,
    pub repo_id: Uuid,
    pub repo_name: String,
    pub repo_display_name: String,
    pub source: WikiSnapshotSource,
    pub wiki: WikiSnapshot,
    pub source_links: Vec<WikiSourceLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct WikiSourceLink {
    pub source: String,
    pub project_id: Uuid,
    pub issue_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
pub enum WikiSnapshotSource {
    CurrentWorkspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UpdateWikiConfigRequest {
    pub repo_id: Uuid,
    pub output_language: String,
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/", get(get_snapshot))
        .route("/config", put(update_config))
}

fn map_wiki_error(error: WikiError) -> ApiError {
    match error {
        WikiError::Io(error) => ApiError::Io(error),
        other => ApiError::BadRequest(other.to_string()),
    }
}

struct ResolvedWikiRepo {
    id: Uuid,
    name: String,
    display_name: String,
    root: PathBuf,
}

async fn resolve_wiki_repo(
    deployment: &DeploymentImpl,
    workspace: &Workspace,
    repo_id: Uuid,
) -> Result<ResolvedWikiRepo, ApiError> {
    if WorkspaceRepo::find_by_workspace_and_repo_id(&deployment.db().pool, workspace.id, repo_id)
        .await?
        .is_some()
    {
        let resolved = resolve_workspace_repo_root(deployment, workspace, repo_id).await?;
        return Ok(ResolvedWikiRepo {
            id: resolved.repo.id,
            name: resolved.repo.name,
            display_name: resolved.repo.display_name,
            root: resolved.canonical_repo_root,
        });
    }

    let attached = WorkspaceRepo::find_by_workspace_id(&deployment.db().pool, workspace.id).await?;
    if !workspace.is_direct_folder() || !attached.is_empty() || repo_id != workspace.id {
        return Err(ApiError::BadRequest(
            "Repository is not part of this workspace".to_string(),
        ));
    }
    let container_ref = deployment
        .container()
        .ensure_container_exists(workspace)
        .await?;
    let root = tokio::fs::canonicalize(container_ref).await?;
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("direct-folder")
        .to_string();
    Ok(ResolvedWikiRepo {
        id: workspace.id,
        display_name: workspace.name.clone().unwrap_or_else(|| name.clone()),
        name,
        root,
    })
}

async fn resolve_source_links(
    deployment: &DeploymentImpl,
    wiki: &WikiSnapshot,
) -> Result<Vec<WikiSourceLink>, ApiError> {
    let mut sources = wiki
        .pages
        .iter()
        .filter_map(|page| page.metadata.as_ref())
        .flat_map(|metadata| metadata.sources.iter())
        .cloned()
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    sources.truncate(MAX_SOURCE_LINKS);
    let mut links = Vec::new();
    for source in sources {
        let row = sqlx::query_as::<_, (Uuid, Uuid)>(
            "SELECT project_id, id FROM local_issues WHERE simple_id = ? LIMIT 1",
        )
        .bind(&source)
        .fetch_optional(&deployment.db().pool)
        .await?;
        if let Some((project_id, issue_id)) = row {
            links.push(WikiSourceLink {
                source,
                project_id,
                issue_id,
            });
        }
    }
    Ok(links)
}

pub async fn get_snapshot(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<WikiRepoQuery>,
) -> Result<ResponseJson<ApiResponse<WorkspaceWikiSnapshot>>, ApiError> {
    let resolved = resolve_wiki_repo(&deployment, &workspace, query.repo_id).await?;
    let root = resolved.root.clone();
    let wiki = tokio::task::spawn_blocking(move || {
        if query.openwiki {
            wiki::openwiki::load_snapshot(&root)
        } else {
            wiki::load_snapshot(&root)
        }
    })
    .await
    .map_err(|error| ApiError::BadRequest(error.to_string()))?
    .map_err(map_wiki_error)?;
    let source_links = resolve_source_links(&deployment, &wiki).await?;
    Ok(ResponseJson(ApiResponse::success(WorkspaceWikiSnapshot {
        workspace_id: workspace.id,
        repo_id: resolved.id,
        repo_name: resolved.name,
        repo_display_name: resolved.display_name,
        source: WikiSnapshotSource::CurrentWorkspace,
        wiki,
        source_links,
    })))
}

pub async fn update_config(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(body): Json<UpdateWikiConfigRequest>,
) -> Result<ResponseJson<ApiResponse<WorkspaceWikiSnapshot>>, ApiError> {
    let resolved = resolve_wiki_repo(&deployment, &workspace, body.repo_id).await?;
    let root = resolved.root.clone();
    let output_language = body.output_language.trim().to_string();
    let wiki = tokio::task::spawn_blocking(move || {
        wiki::update_config(&root, &output_language)?;
        wiki::load_snapshot(&root)
    })
    .await
    .map_err(|error| ApiError::BadRequest(error.to_string()))?
    .map_err(map_wiki_error)?;
    let source_links = resolve_source_links(&deployment, &wiki).await?;
    Ok(ResponseJson(ApiResponse::success(WorkspaceWikiSnapshot {
        workspace_id: workspace.id,
        repo_id: resolved.id,
        repo_name: resolved.name,
        repo_display_name: resolved.display_name,
        source: WikiSnapshotSource::CurrentWorkspace,
        wiki,
        source_links,
    })))
}
