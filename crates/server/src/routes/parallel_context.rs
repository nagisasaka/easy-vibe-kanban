//! Local repository context and read-only pinned source access. Preview creates
//! only a retained detached trial, never promotes source or changes Card state.
use std::path::Path;

use axum::{
    Extension, Json,
    extract::{Path as ApiPath, Query, State},
};
use db::models::{repo::Repo, workspace::Workspace, workspace_repo::WorkspaceRepo};
use deployment::Deployment;
use git::GitService;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub struct SnapshotQuery {
    pub commit: String,
    pub path: Option<String>,
    pub after_path: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct PinnedFile {
    pub path: String,
    pub object_id: String,
    pub mode: i32,
    pub content: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct PinnedSnapshot {
    pub repository_id: Uuid,
    pub commit: String,
    pub files: Vec<PinnedFile>,
    pub next_after: Option<String>,
}

pub async fn snapshot(
    State(deployment): State<DeploymentImpl>,
    ApiPath(id): ApiPath<Uuid>,
    Query(query): Query<SnapshotQuery>,
) -> Result<Json<ApiResponse<PinnedSnapshot>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, id)
        .await?;
    let result = tokio::task::spawn_blocking(move || -> Result<_, git::GitServiceError> {
        let reader = GitService::new().snapshot(&repo.path, &query.commit)?;
        let (entries, next_after) = if let Some(path) = &query.path {
            (vec![reader.entry(path)?], None)
        } else {
            let mut entries: Vec<_> = reader.entries()?.into_iter()
                .filter(|entry| query.after_path.as_ref().is_none_or(|after| &entry.path > after))
                .take(101).collect();
            let next = (entries.len() > 100).then(|| entries[99].path.clone());
            entries.truncate(100);
            (entries, next)
        };
        let mut files = Vec::new();
        for entry in entries {
            let (content, unavailable_reason) = if query.path.is_none() { (None, None) }
            else if !matches!(entry.mode, 0o100644 | 0o100755) { (None, Some("Not a regular file; symlinks and submodules are not followed".into())) }
            else { match reader.read_blob(&entry, 128 * 1024)? {
                None => (None, Some("File exceeds inline read bound; read this pinned Git object in segments with Git".into())),
                Some(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => (Some(text), None),
                    Err(_) => (None, Some("Not UTF-8 text".into())),
                },
            }};
            files.push(PinnedFile { path:entry.path, object_id:entry.oid, mode:entry.mode, content, unavailable_reason });
        }
        Ok(PinnedSnapshot { repository_id:id, commit:query.commit, files, next_after })
    }).await.map_err(|error| ApiError::BadRequest(error.to_string()))??;
    Ok(Json(ApiResponse::success(result)))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PeerSnapshot {
    pub workspace_id: Uuid,
    pub commit: String,
}

#[derive(Debug, Deserialize, TS)]
pub struct CreatePreview {
    pub repository_id: Uuid,
    pub base_commit: String,
    pub peers: Vec<PeerSnapshot>,
}

#[derive(Debug, Serialize, TS)]
pub struct CombinationPreview {
    pub repository_id: Uuid,
    pub owner_workspace_id: Uuid,
    pub path: String,
    pub base_commit: String,
    pub peers: Vec<PeerSnapshot>,
    pub instructions: String,
}

pub async fn preview(
    State(deployment): State<DeploymentImpl>,
    Extension(workspace): Extension<Workspace>,
    Json(request): Json<CreatePreview>,
) -> Result<Json<ApiResponse<CombinationPreview>>, ApiError> {
    let pool = &deployment.db().pool;
    db::models::integration::guard_workspace(pool, workspace.id).await?;
    let repo = Repo::find_by_id(pool, request.repository_id)
        .await?
        .ok_or(db::models::repo::RepoError::NotFound)?;
    let mut sources = request.peers.clone();
    sources.push(PeerSnapshot {
        workspace_id: workspace.id,
        commit: request.base_commit.clone(),
    });
    if sources.len() > 100 {
        return Err(ApiError::BadRequest("Too many preview sources".into()));
    }
    for source in &sources {
        if WorkspaceRepo::find_by_workspace_and_repo_id(pool, source.workspace_id, repo.id)
            .await?
            .is_none()
        {
            return Err(ApiError::BadRequest(
                "Preview source belongs to another repository".into(),
            ));
        }
        let peer = Workspace::find_by_id(pool, source.workspace_id)
            .await?
            .ok_or(db::models::workspace::WorkspaceError::WorkspaceNotFound)?;
        let head = deployment.git().get_branch_oid(&repo.path, &peer.branch)?;
        // Allow older pinned commits from this source, never chase a new HEAD.
        deployment.git().snapshot(&repo.path, &source.commit)?;
        if !deployment
            .git()
            .is_ancestor(&repo.path, &source.commit, &head)?
        {
            return Err(ApiError::BadRequest("Pinned preview commit is no longer attributable to its source Workspace; reselect explicitly".into()));
        }
    }
    // A's own uncommitted changes are not silently omitted from a requested
    // combination. Dirty direct-folder sources likewise require explicit commit.
    let container = workspace.container_ref.as_ref().ok_or_else(|| {
        ApiError::BadRequest(
            "Preview owner checkout is missing; commit or restore it before selecting a snapshot"
                .into(),
        )
    })?;
    let root = if workspace.is_direct_folder() {
        Path::new(container).to_path_buf()
    } else {
        Path::new(container).join(&repo.name)
    };
    deployment
        .git()
        .require_clean_source(&root, &request.base_commit)?;
    let path = worktree_manager::WorktreeManager::create_detached_preview(
        &repo.path,
        &request.base_commit,
    )
    .await?;
    Ok(Json(ApiResponse::success(CombinationPreview {
        repository_id:repo.id, owner_workspace_id:workspace.id,
        path:path.to_string_lossy().into_owned(), base_commit:request.base_commit, peers:request.peers,
        instructions:"Retained detached test worktree: combine only the pinned peer commits here and run tests here. No source/target refs or Card status were changed. Never use reset/stash on the original Workspace. Do not finalise trial changes as normal source, invoke Wiki writers, or modify peers. If a peer itself needs changes, report peer change required with evidence. Keep test-only commands in this directory; explicit cleanup is available through normal git worktree remove after inspection.".into(),
    })))
}
