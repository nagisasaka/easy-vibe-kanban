//! Repository maintenance uses ordinary EVK workspace/session/AgentRun owners.
//! The shared durable pointer and OS lock fence writers across server restarts.
pub(crate) mod completion;
use std::{
    io::Write,
    path::{Path as FsPath, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};
use axum::{
    Json,
    extract::{Path, State},
};
use db::models::{
    agent_runtime::{AgentRunRecord, NativeAuditStreamRecord},
    pull_request::PullRequest,
    repo::Repo,
    session::{CreateSession, Session},
    workspace::Workspace,
    workspace_repo::{CreateWorkspaceRepo, WorkspaceRepo},
};
use deployment::Deployment;
use executors::{
    executors::BaseCodingAgent,
    profile::ExecutorConfig,
    runtime::{AgentRunStatus, NativeAuditDirection, NativeAuditReader},
};
use serde::Deserialize;
use services::services::{
    container::ContainerService,
    openwiki::{HostReconciliationProof, OpenWikiAdapter},
    repository_memory::{pending_events, recover_integrations, unresolved_integration},
};
use ts_rs::TS;
use utils::{
    repository_memory::{
        ReconciliationResult, RepositoryMemoryState, RepositoryMemoryStore, RepositoryWikiStatus,
        WikiReconciliationReceipt, reject_symlinks,
    },
    response::ApiResponse,
};
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize, TS)]
pub struct ConfigureRepositoryMemory {
    pub enabled: bool,
    pub target_branch: String,
    pub output_language: String,
}

fn api_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(error.to_string())
}

async fn repo(deployment: &DeploymentImpl, id: Uuid) -> Result<Repo, ApiError> {
    Repo::find_by_id(&deployment.db().pool, id)
        .await?
        .ok_or_else(|| api_error("Repository not found"))
}

pub async fn get_status(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RepositoryMemoryState>>, ApiError> {
    let repo = repo(&deployment, id).await?;
    let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, id)? else {
        return Ok(Json(ApiResponse::success(RepositoryMemoryState::default())));
    };
    let mut state = store.state()?;
    state.coding_errors = store.coding_errors()?;
    if let Some(branch) = &state.target_branch {
        let head = deployment.git().get_branch_oid(&repo.path, branch)?;
        let pending = unresolved_integration(&store).map_err(api_error)?
            || !pending_events(&store, branch)
                .map_err(api_error)?
                .is_empty();
        let source_matches = state
            .wiki_commit
            .as_deref()
            .or(state.source_commit.as_deref())
            == Some(&head);
        let exists = services::services::openwiki::has_canonical_wiki(&repo.path, &head)
            .map_err(api_error)?;
        state.status = state.derived_status(exists, pending, source_matches);
    }
    Ok(Json(ApiResponse::success(state)))
}

pub async fn configure(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Json(request): Json<ConfigureRepositoryMemory>,
) -> Result<Json<ApiResponse<RepositoryMemoryState>>, ApiError> {
    let repo = repo(&deployment, id).await?;
    deployment
        .git()
        .get_branch_oid(&repo.path, &request.target_branch)?;
    if !services::services::wiki::is_valid_language_tag(&request.output_language) {
        return Err(api_error("Use a valid BCP 47 language code"));
    }
    workspace_manager::shared_resources::ensure(&repo.path, &repo.name, repo.id)?;
    let store = RepositoryMemoryStore::for_repository(&repo.name, repo.id)?;
    let _lock = store.try_lock()?;
    let mut state = store.state()?;
    if state.active_run_id.is_some() || state.bootstrap.is_some() {
        return Err(api_error(
            "Repository maintenance is active; stop or finish that AgentRun before changing configuration",
        ));
    }
    let changed = state.target_branch.as_deref() != Some(&request.target_branch)
        || state.output_language != request.output_language;
    state.enabled = request.enabled;
    state.target_branch = Some(request.target_branch);
    state.output_language = request.output_language;
    if !state.enabled {
        state.status = RepositoryWikiStatus::Disabled;
    } else if state.last_success.is_none() {
        state.status = RepositoryWikiStatus::Uninitialized;
    } else if changed || state.status == RepositoryWikiStatus::Disabled {
        state.status = RepositoryWikiStatus::Stale;
    }
    if state.status != RepositoryWikiStatus::Error {
        state.error = None;
    }
    store.save_state(&state)?;
    Ok(Json(ApiResponse::success(state)))
}

pub async fn sync(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<RepositoryMemoryState>>, ApiError> {
    start_reconciliation(&deployment, id)
        .await
        .map_err(api_error)?;
    get_status(State(deployment), Path(id)).await
}

/// Reserve before activation so recovery always knows the exact process owner.
pub async fn start_reconciliation(deployment: &DeploymentImpl, id: Uuid) -> anyhow::Result<()> {
    let repo = repo(deployment, id).await?;
    let store = RepositoryMemoryStore::for_repository(&repo.name, id)?;
    let _lock = store
        .try_lock()
        .context("Another repository maintainer is starting or publishing")?;
    let mut state = store.state()?;
    if !state.enabled {
        bail!("Enable OpenWiki repository memory first");
    }
    if state.active_run_id.is_some() || state.bootstrap.is_some() {
        bail!(
            "Repository maintenance already has an active AgentRun; inspect its workspace or wait for recovery"
        );
    }
    let result = prepare_run(deployment, &repo, &store, &mut state).await;
    if let Err(error) = result {
        state = store.state()?;
        state.status = RepositoryWikiStatus::Error;
        state.error = Some(format!("{error:#}"));
        if let Some(owner) = &mut state.bootstrap {
            owner.phase = utils::repository_memory::OpenWikiBootstrapPhase::CleaningUp;
        }
        store.save_state(&state)?;
        return Err(error);
    }
    Ok(())
}

async fn prepare_run(
    deployment: &DeploymentImpl,
    repo: &Repo,
    store: &RepositoryMemoryStore,
    state: &mut RepositoryMemoryState,
) -> anyhow::Result<()> {
    let branch = state
        .target_branch
        .clone()
        .context("Choose an integration branch")?;
    recover_integrations(store, &repo.path)?;
    if unresolved_integration(store)? {
        bail!(
            "A source integration has no confirmed outcome. Finish/retry that source merge before Wiki reconciliation; its semantic events have not been acknowledged."
        );
    }
    if deployment.git().is_remote_branch(&repo.path, &branch)? {
        bail!("Wiki publication requires a local integrated branch; fetch/integrate source first");
    }
    let source = deployment.git().get_branch_oid(&repo.path, &branch)?;
    for integration in store
        .integrations()?
        .into_iter()
        .filter(|record| record.target_branch == branch)
    {
        if let Some(commit) = integration.integrated_commit
            && integration.event_ids.iter().any(|id| {
                store
                    .receipt(*id)
                    .ok()
                    .flatten()
                    .is_none_or(|receipt| receipt.result == ReconciliationResult::Failed)
            })
            && !deployment
                .git()
                .is_ancestor(&repo.path, &commit, &source)
                .unwrap_or(false)
        {
            bail!(
                "A merged source change is not available on the selected local branch. Fetch and integrate that source before Sync Wiki; no model run was started."
            );
        }
    }
    OpenWikiAdapter::default()
        .verify_version(&repo.path)
        .await?;
    let workspace = super::workspaces::create::create_workspace_record(
        deployment,
        Some(format!("OpenWiki: {}", repo.display_name)),
    )
    .await?;
    WorkspaceRepo::create_many(
        &deployment.db().pool,
        workspace.id,
        &[CreateWorkspaceRepo {
            repo_id: repo.id,
            target_branch: branch.clone(),
        }],
    )
    .await?;
    let container = deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;
    let root = PathBuf::from(&container).join(&repo.name);
    if deployment.git().get_head_info(&root)?.oid != source {
        bail!("Integrated source advanced during maintenance preparation; retry");
    }
    if !deployment
        .git()
        .get_diff_file_paths(&root, &source.parse()?)?
        .is_empty()
    {
        bail!(
            "Maintenance setup changed repository files before OpenWiki started. Inspect startup/copy settings; no model run was started."
        );
    }
    let project_scope = OpenWikiAdapter::requires_project_scope();
    services::services::openwiki::setup::preflight(&root)?;
    OpenWikiAdapter::default()
        .prepare_codex(&root, project_scope)
        .await?;
    reject_symlinks(&root.join("openwiki/INSTRUCTIONS.md"))?;
    std::fs::create_dir_all(root.join("openwiki"))?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("openwiki/INSTRUCTIONS.md"))
    {
        Ok(mut file) => {
            file.write_all(include_bytes!(
                "../../../../assets/openwiki-instructions.md"
            ))?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let events = pending_events(store, &branch)?;
    let mut memories = Vec::new();
    let workspace_ids: std::collections::BTreeSet<_> =
        events.iter().map(|event| event.workspace_id).collect();
    for workspace_id in workspace_ids {
        if let Some(memory) = store.read_memory(workspace_id)? {
            memories
                .push(serde_json::json!({"workspace_id": workspace_id, "semantic_memory": memory}));
        }
    }
    let hints = serde_json::to_string_pretty(
        &serde_json::json!({"change_manifests": events, "workspace_memories": memories}),
    )?;
    // First bootstrap is explicit init; subsequent sync preserves existing Wiki.
    let initial = !root.join("openwiki/index.md").exists();
    if initial {
        state.active_source_commit = Some(source.clone());
        state.active_event_ids = events.iter().map(|event| event.event_id).collect();
        state.error = None;
        return crate::workflow_runtime::bootstrap::start(
            deployment, repo, store, state, &workspace, &root,
        )
        .await;
    }
    let prompt =
        OpenWikiAdapter::maintenance_prompt(&root, initial, &state.output_language, &hints);
    let skill_path = OpenWikiAdapter::installed_skill_path(&root, project_scope)?;
    let session = Session::create(
        &deployment.db().pool,
        &CreateSession {
            executor: Some("CODEX".into()),
            name: Some("OpenWiki repository reconciliation".into()),
        },
        Uuid::new_v4(),
        workspace.id,
    )
    .await?;
    state.status = if initial {
        RepositoryWikiStatus::Initializing
    } else {
        RepositoryWikiStatus::Reconciling
    };
    state.maintenance_workspace_id = Some(workspace.id);
    state.maintenance_session_id = Some(session.id);
    state.active_source_commit = Some(source.clone());
    state.active_event_ids = events.iter().map(|event| event.event_id).collect();
    state.error = None;
    store.save_state(state)?;
    if let Err(error) =
        services::services::openwiki::setup::prepare(store, workspace.id, &root, &source)
    {
        // No AgentRun has been reserved yet. A partially materialised snapshot
        // can be restored safely; never do this once a provider may be running.
        services::services::openwiki::setup::restore(store, workspace.id, &root, &source)
            .context("OpenWiki pre-launch instruction restoration failed")?;
        return Err(error);
    }
    let snapshot = super::sessions::reserve_coding_agent_execution_for_session(
        deployment,
        session,
        prompt,
        Some(vec![executors::actions::SelectedSkill {
            name: "openwiki".into(),
            path: skill_path,
        }]),
        ExecutorConfig::new(BaseCodingAgent::Codex),
        None,
        None,
    )
    .await;
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => {
            services::services::openwiki::setup::restore(store, workspace.id, &root, &source)
                .context("OpenWiki reservation failed and instructions could not be restored")?;
            return Err(error.into());
        }
    };
    state.active_run_id = Some(snapshot.agent_run_id);
    store.save_state(state)?;
    super::sessions::launch_reserved_coding_agent_execution(deployment, snapshot.agent_run_id)
        .await?;
    Ok(())
}

/// Native Audit checksum validation precedes inspection; only root-thread
/// OpenWiki MCP completion frames can acknowledge semantic events.
pub(crate) async fn completion_proof(
    deployment: &DeploymentImpl,
    run: &AgentRunRecord,
    root: &FsPath,
) -> anyhow::Result<HostReconciliationProof> {
    let stream: NativeAuditStreamRecord = sqlx::query_as("SELECT * FROM native_audit_streams WHERE agent_run_id = ? ORDER BY created_at DESC LIMIT 1")
        .bind(run.id).fetch_one(&deployment.db().pool).await?;
    let path = utils::assets::asset_dir().join(&stream.manifest_relative_path);
    let audit = NativeAuditReader::read(path.parent().context("Invalid audit path")?)?;
    let provider_thread: String = sqlx::query_scalar(
        "SELECT provider_session_id FROM agent_provider_sessions WHERE session_id = ?",
    )
    .bind(run.session_id)
    .fetch_one(&deployment.db().pool)
    .await?;
    let mut proof = HostReconciliationProof::default();
    for frame in audit.frames() {
        if frame.direction != NativeAuditDirection::Output {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&frame.payload_bytes()?) else {
            continue;
        };
        proof.observe_codex_frame(&value, &provider_thread, root)?;
    }
    if !proof.complete {
        bail!(
            "Codex exited without a verified OpenWiki begin-noop or finish-complete result. Inspect the maintenance session and retry; pending events were not acknowledged."
        );
    }
    Ok(proof)
}

async fn recover_repository(deployment: &DeploymentImpl, repo: &Repo) -> anyhow::Result<()> {
    let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)? else {
        return Ok(());
    };
    let Ok(_lock) = store.try_lock() else {
        return Ok(());
    };
    let mut state = store.state()?;
    observe_recorded_pr_integrations(deployment, repo, &store).await?;
    recover_integrations(&store, &repo.path)?;
    if state.bootstrap.is_some() {
        return crate::workflow_runtime::bootstrap::recover_locked(deployment, repo, &store).await;
    }
    let Some(run_id) = state.active_run_id else {
        // Initialisation requires an explicit request. After that, merged
        // source events trigger maintenance; errors require an explicit retry
        // rather than an unbounded series of paid model runs.
        let needs_sync = state.enabled
            && state.last_success.is_some()
            && state.status != RepositoryWikiStatus::Error
            && state
                .target_branch
                .as_deref()
                .map(|branch| pending_events(&store, branch))
                .transpose()?
                .is_some_and(|events| !events.is_empty());
        drop(_lock);
        if needs_sync {
            start_reconciliation(deployment, repo.id).await?;
        }
        return Ok(());
    };
    let run = AgentRunRecord::find(&deployment.db().pool, run_id)
        .await?
        .context("Maintenance AgentRun identity is missing; keep writer fenced")?;
    if !run.status.is_terminal() {
        // Reservation may have survived a server restart before activation.
        if run.status == AgentRunStatus::Pending {
            super::sessions::launch_reserved_coding_agent_execution(deployment, run_id).await?;
        }
        return Ok(());
    }
    let result = finish_run(deployment, repo, &store, &state, &run).await;
    if result
        .as_ref()
        .err()
        .and_then(|error| error.downcast_ref::<std::io::Error>())
        .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
    {
        // Another source merge holds the short publication lock. Keep the
        // successful host owner/checkpoint and retry; never pay for another run.
        return Ok(());
    }
    record_reconciliation_result(&store, &mut state, result)
}

/// Shared receipt/freshness completion for the single-host Sync and the
/// Workflow-owned Bootstrap. Call only after publication or confirmed cleanup.
pub(crate) fn record_reconciliation_result(
    store: &RepositoryMemoryStore,
    state: &mut RepositoryMemoryState,
    result: anyhow::Result<(Option<String>, bool)>,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now();
    match result {
        Ok((wiki_commit, no_op)) => {
            for id in &state.active_event_ids {
                store.acknowledge(&WikiReconciliationReceipt {
                    event_id: *id,
                    reconciled_at: now,
                    target_commit: state
                        .active_source_commit
                        .clone()
                        .context("missing source")?,
                    wiki_commit: wiki_commit.clone(),
                    result: if no_op {
                        ReconciliationResult::NoOp
                    } else {
                        ReconciliationResult::Updated
                    },
                    error: None,
                })?;
            }
            state.source_commit = state.active_source_commit.clone();
            state.wiki_commit = wiki_commit;
            state.last_success = Some(now);
            state.status = RepositoryWikiStatus::Current;
            state.error = None;
        }
        Err(error) => {
            state.status = RepositoryWikiStatus::Error;
            state.error = Some(format!("{error:#}"));
            for id in &state.active_event_ids {
                store.acknowledge(&WikiReconciliationReceipt {
                    event_id: *id,
                    reconciled_at: now,
                    target_commit: state
                        .active_source_commit
                        .clone()
                        .context("missing source")?,
                    wiki_commit: None,
                    result: ReconciliationResult::Failed,
                    error: state.error.clone(),
                })?;
            }
        }
    }
    state.active_run_id = None;
    state.bootstrap = None;
    state.active_event_ids.clear();
    state.active_source_commit = None;
    store.save_state(state)?;
    Ok(())
}

/// Reuse the PR monitor's durable results, without new network requests. A PR
/// commit can be known before it is fetched; preparation separately verifies
/// that the local integrated branch contains it before launching a model.
async fn observe_recorded_pr_integrations(
    deployment: &DeploymentImpl,
    repo: &Repo,
    store: &RepositoryMemoryStore,
) -> anyhow::Result<()> {
    let prs: Vec<PullRequest> = sqlx::query_as("SELECT * FROM pull_requests WHERE repo_id = ? AND pr_status = 'merged' AND merge_commit_sha IS NOT NULL")
        .bind(repo.id).fetch_all(&deployment.db().pool).await?;
    for pr in prs {
        let id = Uuid::parse_str(&pr.id)?;
        let (Some(workspace_id), Some(merge_commit)) = (pr.workspace_id, pr.merge_commit_sha)
        else {
            continue;
        };
        services::services::repository_memory::observe_external_integration(
            store,
            &repo.path,
            id,
            workspace_id,
            &pr.target_branch_name,
            &merge_commit,
        )?;
    }
    Ok(())
}

async fn finish_run(
    deployment: &DeploymentImpl,
    repo: &Repo,
    store: &RepositoryMemoryStore,
    state: &RepositoryMemoryState,
    run: &AgentRunRecord,
) -> anyhow::Result<(Option<String>, bool)> {
    if !run.status.is_terminal() {
        bail!("Cannot restore OpenWiki instructions while its AgentRun is active");
    }
    let root = PathBuf::from(&run.workspace_path).join(&repo.name);
    let source = state
        .active_source_commit
        .as_deref()
        .context("Missing source checkpoint")?;
    // Wait for the host to exit before changing its fingerprinted source view.
    // Failed/cancelled hosts get their original instructions back too, but can
    // never publish their partial Wiki. A durable publication is already clean.
    if store.publication(run.id)?.is_none() {
        let completion = if run.status == AgentRunStatus::Succeeded {
            completion_proof(deployment, run, &root).await.map(|_| ())
        } else {
            Err(anyhow::anyhow!(
                "OpenWiki AgentRun ended {:?}; generated files remain in its workspace for inspection",
                run.status
            ))
        };
        let provenance = if completion.is_ok() {
            services::services::openwiki::setup::validate_provenance(
                store,
                run.workspace_id,
                &root,
                source,
            )
        } else {
            Ok(())
        };
        services::services::openwiki::setup::restore(store, run.workspace_id, &root, source)?;
        completion?;
        provenance?;
        services::services::openwiki::setup::validate_provenance(
            store,
            run.workspace_id,
            &root,
            source,
        )?;
    } else if run.status != AgentRunStatus::Succeeded {
        bail!("A failed AgentRun cannot publish an OpenWiki checkpoint");
    }
    let branch = state
        .target_branch
        .as_deref()
        .context("Missing target branch")?;
    let workspace = Workspace::find_by_id(&deployment.db().pool, run.workspace_id)
        .await?
        .context("Maintenance workspace missing")?;
    let request = services::services::openwiki::WikiPublicationRequest {
        repository_root: &repo.path,
        maintenance_root: &root,
        maintenance_branch: &workspace.branch,
        target_branch: branch,
        source_commit: source,
        run_id: run.id,
    };
    let result =
        services::services::openwiki::publish_validated_wiki(deployment.git(), store, &request);
    if result.is_err() && store.publication(run.id)?.is_some() {
        // Git may report failure after committing the target but before updating
        // its task ref. One bounded replay resolves that exact publication ID.
        return services::services::openwiki::publish_validated_wiki(
            deployment.git(),
            store,
            &request,
        );
    }
    result
}

pub fn spawn_recovery_monitor(deployment: DeploymentImpl) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let repos = match Repo::list_all(&deployment.db().pool).await {
                Ok(repos) => repos,
                Err(error) => {
                    tracing::warn!(%error, "OpenWiki recovery repository lookup failed");
                    continue;
                }
            };
            for repo in repos {
                if let Err(error) = recover_repository(&deployment, &repo).await {
                    tracing::warn!(repository_id = %repo.id, %error, "OpenWiki recovery deferred");
                }
            }
        }
    });
}
