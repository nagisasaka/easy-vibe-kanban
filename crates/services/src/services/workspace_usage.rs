//! Workspace usage policy shared by admission, dispatch and inspection.
//! Product owners retain their existing state machines; attribution is not a
//! capability supplied by clients and it never expires with a dispatcher lease.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use db::models::{
    repo::Repo,
    workspace::Workspace,
    workspace_repo::WorkspaceRepo,
    workspace_usage::{INTEGRATION, OPENWIKI_BOOTSTRAP, OPENWIKI_SYNC, WorkspaceExecutionOwner},
};
use executors::runtime::AgentRunRequestEnvelope;
use sqlx::{SqlitePool, types::Json};
use utils::repository_memory::{RepositoryMemoryStore, RepositoryWikiStatus};
use uuid::Uuid;

/// Only existing, host-owned records establish historical usage. Ordinary
/// workflows, unlinked workspaces and names are deliberately not evidence.
pub async fn backfill(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut evidence: BTreeMap<Uuid, Vec<WorkspaceExecutionOwner>> = BTreeMap::new();
    let rows: Vec<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT workspace_id,id,repository_id,'integration' FROM integration_runs WHERE workspace_id IS NOT NULL
         UNION ALL SELECT workspace_id,id,repository_id,'openwiki_bootstrap' FROM workflow_runs
         WHERE trigger_source='openwiki_bootstrap' AND issue_id IS NULL AND attempt_id IS NULL
           AND repository_id IS NOT NULL AND workspace_id IS NOT NULL"
    ).fetch_all(pool).await?;
    for (ws, run, repo, kind) in rows {
        evidence
            .entry(ws)
            .or_default()
            .push(WorkspaceExecutionOwner::new(&kind, repo, Some(run)));
    }
    let mut unproven = 0;
    for (id,) in sqlx::query_as::<_, (Uuid,)>(
        "SELECT id FROM workspaces WHERE usage='interactive' AND execution_owner IS NULL",
    )
    .fetch_all(pool)
    .await?
    {
        if evidence.contains_key(&id) {
            continue;
        }
        let workspace = Workspace::find_by_id(pool, id)
            .await?
            .context("Workspace disappeared during usage backfill")?;
        for repo in WorkspaceRepo::find_repos_for_workspace(pool, id).await? {
            let store = match RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id) {
                Ok(Some(store)) => store,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(%id, %error, "Unreadable historical repository memory; preserving usage");
                    continue;
                }
            };
            let setup = match store.wiki_setup(id) {
                Ok(Some(setup)) => setup,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(%id, %error, "Unreadable historical Wiki setup; preserving usage");
                    continue;
                }
            };
            let expected = workspace
                .container_ref
                .as_ref()
                .map(|root| Path::new(root).join(&repo.name));
            if setup.workspace_id != id
                || expected.as_deref() != Some(Path::new(&setup.repository_path))
            {
                tracing::warn!(%id, "Wiki setup path identity mismatch; preserving usage");
                continue;
            }
            let runs: Vec<(Uuid,)> =
                sqlx::query_as("SELECT id FROM agent_runs WHERE workspace_id=?")
                    .bind(id)
                    .fetch_all(pool)
                    .await?;
            // A setup checkpoint proves internal purpose, but multiple unrelated
            // runs do not establish which one published. Keep that outcome unknown.
            let run_id = if runs.len() == 1 {
                Some(runs[0].0)
            } else {
                None
            };
            evidence
                .entry(id)
                .or_default()
                .push(WorkspaceExecutionOwner::new(OPENWIKI_SYNC, repo.id, run_id));
        }
        if !evidence.contains_key(&id) {
            unproven += 1;
        }
    }
    let mut classified = 0;
    let mut conflicts = 0;
    for (id, mut owners) in evidence {
        let mut unique = Vec::new();
        for owner in owners {
            if !unique.contains(&owner) {
                unique.push(owner);
            }
        }
        owners = unique;
        let owner = if owners.len() == 1 {
            owners.remove(0)
        } else {
            conflicts += 1;
            tracing::error!(%id, ?owners, "Conflicting execution ownership; inspection only");
            WorkspaceExecutionOwner {
                kind: "conflicting_evidence".into(),
                run_id: None,
                repository_id: None,
                result: None,
            }
        };
        classified += sqlx::query("UPDATE workspaces SET usage='execution_only',execution_owner=? WHERE id=? AND usage='interactive' AND execution_owner IS NULL")
            .bind(Json(owner)).bind(id).execute(pool).await?.rows_affected();
    }
    tracing::info!(
        classified,
        conflicts,
        without_internal_evidence = unproven,
        "Workspace usage backfill (unproven workspaces remain interactive; not inferred from names)"
    );
    Ok(())
}

/// Reserve/dispatch checks complement (never replace) the narrower phase and
/// executor validation performed by the owning runtime.
pub async fn validate_agent_owner(
    pool: &SqlitePool,
    workspace: &Workspace,
    request: &AgentRunRequestEnvelope,
    launching: bool,
) -> anyhow::Result<()> {
    if !workspace.is_execution_only() {
        return Ok(());
    }
    let owner = workspace
        .execution_owner
        .as_ref()
        .context("Execution-only workspace has no owner; inspect its history")?;
    ensure!(
        owner.result.is_none(),
        "Execution workspace has a terminal owner result"
    );
    let repo_id = owner
        .repository_id
        .context("Execution repository identity is missing")?;
    ensure!(
        WorkspaceRepo::find_by_workspace_and_repo_id(pool, workspace.id, repo_id)
            .await?
            .is_some(),
        "Execution owner repository membership mismatch"
    );
    match owner.kind.as_str() {
        INTEGRATION => {
            let run_id = owner.run_id.context("Integration owner is unbound")?;
            ensure!(
                request.correlation_id == run_id,
                "Integration correlation does not match execution owner"
            );
            let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integration_runs WHERE id=? AND repository_id=? AND workspace_id=? AND session_id=?)")
                .bind(run_id).bind(repo_id).bind(workspace.id).bind(request.session_id).fetch_one(pool).await?;
            ensure!(owned, "Integration execution identity mismatch");
            db::models::integration::guard_agent_dispatch(
                pool,
                workspace.id,
                request.session_id,
                request.correlation_id,
            )
            .await?;
        }
        OPENWIKI_BOOTSTRAP | OPENWIKI_SYNC => {
            let repo = Repo::find_by_id(pool, repo_id)
                .await?
                .context("Execution repository missing")?;
            let store = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)?
                .context("Maintenance owner missing")?;
            let state = store.state()?;
            ensure!(
                state.maintenance_workspace_id == Some(workspace.id)
                    && state.maintenance_session_id == Some(request.session_id),
                "Maintenance execution identity mismatch"
            );
            ensure!(
                matches!(
                    state.status,
                    RepositoryWikiStatus::Initializing | RepositoryWikiStatus::Reconciling
                ),
                "Maintenance is not accepting Agent dispatch"
            );
            if owner.kind == OPENWIKI_BOOTSTRAP {
                let bootstrap = state.bootstrap.context("Bootstrap ownership missing")?;
                ensure!(
                    Some(bootstrap.workflow_run_id) == owner.run_id,
                    "Bootstrap run does not match workspace owner"
                );
                let child = bootstrap
                    .child
                    .context("Bootstrap child has not been delegated")?;
                ensure!(
                    child.session_id == request.session_id
                        && child.agent_run_id == request.agent_run_id,
                    "Bootstrap child identity mismatch"
                );
            } else {
                ensure!(
                    state.bootstrap.is_none(),
                    "Sync cannot borrow Bootstrap ownership"
                );
                ensure!(
                    owner.run_id.is_none_or(|id| id == request.agent_run_id)
                        && state
                            .active_run_id
                            .is_none_or(|id| id == request.agent_run_id),
                    "Sync run identity mismatch"
                );
                if launching {
                    ensure!(
                        owner.run_id == Some(request.agent_run_id)
                            && state.active_run_id == owner.run_id,
                        "Sync ownership must be bound before launch"
                    );
                }
            }
        }
        _ => anyhow::bail!(
            "Unknown execution owner {}; inspect history instead of starting arbitrary work",
            owner.kind
        ),
    }
    Ok(())
}

/// No mkdir, worktree restoration, scripts, or database changes. Callers retain
/// their repository membership and path/symlink checks beneath this root.
pub fn inspection_root(workspace: &Workspace) -> anyhow::Result<PathBuf> {
    ensure!(
        !workspace.worktree_deleted,
        "Execution files were cleaned up; saved logs are still available"
    );
    let root = PathBuf::from(
        workspace
            .container_ref
            .as_ref()
            .context("Execution files have not been prepared")?,
    );
    ensure!(
        root.is_dir(),
        "Execution files are unavailable; viewing does not recreate a worktree"
    );
    Ok(root)
}

pub async fn validate_script_owner(
    pool: &SqlitePool,
    workspace_id: Uuid,
    session_id: Uuid,
    action: &executors::actions::ExecutorAction,
    reason: &db::models::execution_process::ExecutionProcessRunReason,
) -> anyhow::Result<()> {
    use db::models::{execution_process::ExecutionProcessRunReason, integration::IntegrationRun};
    use executors::actions::{ExecutorActionType, script::ScriptContext};
    let ws = Workspace::find_by_id(pool, workspace_id)
        .await?
        .context("Workspace missing")?;
    if !ws.is_execution_only() {
        return Ok(());
    }
    let owner = ws.execution_owner.context("Execution owner missing")?;
    ensure!(
        owner.kind == INTEGRATION && *reason == ExecutionProcessRunReason::IntegrationValidation,
        "Execution-only workspace does not permit arbitrary scripts"
    );
    let run =
        IntegrationRun::find(pool, owner.run_id.context("Execution owner is unbound")?).await?;
    ensure!(
        run.workspace_id == Some(workspace_id)
            && run.session_id == Some(session_id)
            && Some(run.repository_id) == owner.repository_id
            && run.status == "validating"
            && !run.cancel_requested
            && db::models::integration::workspace_owner(pool, workspace_id).await? == Some(run.id),
        "Integration validation ownership mismatch"
    );
    let ExecutorActionType::ScriptRequest(script) = action.typ();
    ensure!(
        script.context == ScriptContext::IntegrationValidation && action.next_action.is_none(),
        "Validation script context mismatch"
    );
    let repo = Repo::find_by_id(pool, run.repository_id)
        .await?
        .context("Repository missing")?;
    ensure!(
        WorkspaceRepo::find_by_workspace_and_repo_id(pool, workspace_id, repo.id)
            .await?
            .is_some(),
        "Validation repository membership mismatch"
    );
    ensure!(
        run.payload
            .validation
            .iter()
            .any(|item| item.command == script.script
                && script.working_dir.as_deref()
                    == Some(
                        Path::new(&repo.name)
                            .join(&item.cwd)
                            .to_string_lossy()
                            .as_ref()
                    )
                && item.result.is_none()),
        "Script does not match host-persisted validation plan"
    );
    Ok(())
}

pub async fn cleanup_allowed(pool: &SqlitePool, workspace_id: Uuid) -> anyhow::Result<bool> {
    let ws = Workspace::find_by_id(pool, workspace_id)
        .await?
        .context("Workspace missing")?;
    if !ws.is_execution_only() {
        return Ok(true);
    }
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_runs WHERE workspace_id=? AND status NOT IN ('succeeded','failed','cancelled','crashed','audit_failed')) OR EXISTS(SELECT 1 FROM agent_process_registry p JOIN agent_run_attempts a ON a.id=p.run_attempt_id JOIN agent_runs r ON r.id=a.agent_run_id WHERE r.workspace_id=? AND p.registry_status IN ('reserved','spawned','running','unreachable')) OR EXISTS(SELECT 1 FROM execution_processes p JOIN sessions s ON s.id=p.session_id WHERE s.workspace_id=? AND p.status='running')")
        .bind(workspace_id).bind(workspace_id).bind(workspace_id).fetch_one(pool).await?;
    if busy {
        return Ok(false);
    }
    let Some(owner) = ws.execution_owner else {
        return Ok(false);
    };
    match owner.kind.as_str() {
        INTEGRATION => {
            let Some(id) = owner.run_id else {
                return Ok(owner.result.is_some());
            };
            let run = db::models::integration::IntegrationRun::find(pool, id).await?;
            Ok(run.workspace_id == Some(workspace_id)
                && matches!(
                    run.status.as_str(),
                    "succeeded" | "failed" | "cancelled" | "blocked"
                ))
        }
        OPENWIKI_BOOTSTRAP | OPENWIKI_SYNC => {
            let Some(repo_id) = owner.repository_id else {
                return Ok(false);
            };
            let Some(repo) = Repo::find_by_id(pool, repo_id).await? else {
                return Ok(false);
            };
            if let Some(store) =
                RepositoryMemoryStore::existing_for_repository(&repo.name, repo_id)?
            {
                let state = store.state()?;
                if state.maintenance_workspace_id == Some(workspace_id)
                    && (state.active_run_id.is_some() || state.bootstrap.is_some())
                {
                    return Ok(false);
                }
            }
            if owner.kind == OPENWIKI_SYNC {
                return Ok(owner.result.is_some());
            }
            let terminal: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_runs WHERE id=? AND workspace_id=? AND status IN ('succeeded','failed','canceled'))")
                .bind(owner.run_id).bind(workspace_id).fetch_one(pool).await?;
            Ok(terminal || owner.result.is_some())
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use db::models::{
        session::{CreateSession, Session},
        workspace::CreateWorkspace,
        workspace_repo::CreateWorkspaceRepo,
    };
    use executors::{
        actions::{
            ExecutorAction, ExecutorActionType,
            script::{ScriptContext, ScriptRequest, ScriptRequestLanguage},
        },
        runtime::*,
    };

    use super::*;

    async fn fixture() -> (SqlitePool, Repo, Uuid) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        let repo = Repo::find_or_create(
            &pool,
            &PathBuf::from(format!("/nonexistent/workspace-usage-{}", Uuid::new_v4())),
            "Usage test",
        )
        .await
        .unwrap();
        let project = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name) VALUES(?,'Usage test')")
            .bind(project)
            .execute(&pool)
            .await
            .unwrap();
        (pool, repo, project)
    }
    async fn workspace(
        pool: &SqlitePool,
        repo: &Repo,
        owner: Option<&WorkspaceExecutionOwner>,
    ) -> Workspace {
        let ws = Workspace::create_with_owner(
            pool,
            &CreateWorkspace {
                branch: Uuid::new_v4().to_string(),
                name: Some("OpenWiki: misleading human name".into()),
            },
            Uuid::new_v4(),
            owner,
        )
        .await
        .unwrap();
        WorkspaceRepo::create_many(
            pool,
            ws.id,
            &[CreateWorkspaceRepo {
                repo_id: repo.id,
                target_branch: "main".into(),
            }],
        )
        .await
        .unwrap();
        ws
    }
    async fn integration(
        pool: &SqlitePool,
        repo: &Repo,
        project: Uuid,
        ws: Uuid,
        id: Uuid,
        session: Option<Uuid>,
        status: &str,
    ) {
        sqlx::query("INSERT INTO integration_runs(id,request_key,project_id,repository_id,storage_identity,target_ref,status,workspace_id,session_id,payload) VALUES(?,?,?,?,?,'refs/heads/test',?,?,?,?)")
            .bind(id).bind(id.to_string()).bind(project).bind(repo.id).bind(repo.path.to_string_lossy().as_ref()).bind(status).bind(ws).bind(session)
            .bind(Json(db::models::integration::IntegrationPayload::default())).execute(pool).await.unwrap();
    }
    fn request(ws: &Workspace, session_id: Uuid, correlation_id: Uuid) -> AgentRunRequestEnvelope {
        AgentRunRequestEnvelope {
            schema_version: 1,
            payload_version: 1,
            request_id: Uuid::new_v4(),
            idempotency_key: Uuid::new_v4().to_string(),
            session_id,
            agent_run_id: Uuid::new_v4(),
            turn_id: Uuid::new_v4(),
            correlation_id,
            intent: AgentRunIntent::Initial,
            runtime_profile_id: "CODEX".into(),
            provider_id: "codex".into(),
            workspace: WorkspaceReference {
                workspace_id: ws.id,
                mode: WorkspaceMode::IsolatedWorktree,
                path: "/nonexistent".into(),
            },
            input: CanonicalMessage {
                message_id: Uuid::new_v4(),
                role: AgentRuntimeMessageRole::User,
                content: "test".into(),
            },
            created_at: chrono::Utc::now(),
        }
    }

    async fn memory_fixture() -> (SqlitePool, Repo, tempfile::TempDir, RepositoryMemoryStore) {
        let (pool, mut repo, _) = fixture().await;
        // Unique shared root using the production lookup, without changing global
        // environment or touching another repository's memory.
        let base = utils::path::shared_resources_dir("test", repo.id)
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(&base).unwrap();
        let suffix = format!("-{}", repo.id);
        let temp = tempfile::Builder::new()
            .prefix("usage-")
            .suffix(&suffix)
            .tempdir_in(base)
            .unwrap();
        repo.name = temp
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_suffix(&suffix)
            .unwrap()
            .to_owned();
        sqlx::query("UPDATE repos SET name=? WHERE id=?")
            .bind(&repo.name)
            .bind(repo.id)
            .execute(&pool)
            .await
            .unwrap();
        std::fs::create_dir(temp.path().join("persistent")).unwrap();
        let store = RepositoryMemoryStore::for_repository(&repo.name, repo.id).unwrap();
        (pool, repo, temp, store)
    }

    #[tokio::test]
    async fn bootstrap_delegates_only_current_child_and_retains_usage_after_owner_release() {
        use utils::repository_memory::{
            OpenWikiBootstrapChild, OpenWikiBootstrapOwner, OpenWikiBootstrapPhase,
            RepositoryMemoryState,
        };
        let (pool, repo, _temp, store) = memory_fixture().await;
        let run_id = Uuid::new_v4();
        let ws = workspace(
            &pool,
            &repo,
            Some(&WorkspaceExecutionOwner::new(
                OPENWIKI_BOOTSTRAP,
                repo.id,
                Some(run_id),
            )),
        )
        .await;
        let mut state = RepositoryMemoryState {
            status: RepositoryWikiStatus::Initializing,
            maintenance_workspace_id: Some(ws.id),
            ..Default::default()
        };
        let mut previous = None;
        for (node, phase) in [
            ("generate", OpenWikiBootstrapPhase::Generating),
            ("review", OpenWikiBootstrapPhase::Reviewing),
            ("refine", OpenWikiBootstrapPhase::Refining),
        ] {
            let req = request(&ws, Uuid::new_v4(), run_id);
            state.maintenance_session_id = Some(req.session_id);
            state.bootstrap = Some(OpenWikiBootstrapOwner {
                workflow_run_id: run_id,
                server_instance_id: Uuid::new_v4(),
                phase,
                review_fingerprint: None,
                child: Some(OpenWikiBootstrapChild {
                    session_id: req.session_id,
                    agent_run_id: req.agent_run_id,
                    node_execution_id: Uuid::new_v4(),
                    node_id: node.into(),
                }),
            });
            let lock = store.try_lock().unwrap();
            store.save_state(&state).unwrap();
            drop(lock);
            validate_agent_owner(&pool, &ws, &req, false).await.unwrap();
            validate_agent_owner(&pool, &ws, &req, true).await.unwrap();
            let mut spoof = req.clone();
            spoof.agent_run_id = Uuid::new_v4();
            assert!(
                validate_agent_owner(&pool, &ws, &spoof, true)
                    .await
                    .is_err()
            );
            if let Some(old) = &previous {
                assert!(validate_agent_owner(&pool, &ws, old, true).await.is_err());
            }
            previous = Some(req);
            assert!(!cleanup_allowed(&pool, ws.id).await.unwrap());
        }
        state.bootstrap = None;
        state.status = RepositoryWikiStatus::Current;
        let lock = store.try_lock().unwrap();
        store.save_state(&state).unwrap();
        drop(lock);
        assert!(
            validate_agent_owner(&pool, &ws, &previous.unwrap(), true)
                .await
                .is_err()
        );
        assert!(
            Workspace::find_by_id(&pool, ws.id)
                .await
                .unwrap()
                .unwrap()
                .is_execution_only()
        );
    }

    #[tokio::test]
    async fn sync_binds_before_launch_and_retained_no_op_result_prevents_reuse() {
        use db::models::workspace_usage::{
            WorkspaceExecutionResult, WorkspaceExecutionTerminalStatus,
        };
        use utils::repository_memory::RepositoryMemoryState;
        let (pool, repo, _temp, store) = memory_fixture().await;
        let mut ws = workspace(
            &pool,
            &repo,
            Some(&WorkspaceExecutionOwner::new(OPENWIKI_SYNC, repo.id, None)),
        )
        .await;
        let req = request(&ws, Uuid::new_v4(), Uuid::new_v4());
        let mut state = RepositoryMemoryState {
            status: RepositoryWikiStatus::Reconciling,
            maintenance_workspace_id: Some(ws.id),
            maintenance_session_id: Some(req.session_id),
            ..Default::default()
        };
        let lock = store.try_lock().unwrap();
        store.save_state(&state).unwrap();
        drop(lock);
        validate_agent_owner(&pool, &ws, &req, false).await.unwrap();
        assert!(validate_agent_owner(&pool, &ws, &req, true).await.is_err());
        Workspace::bind_execution_owner(&pool, ws.id, OPENWIKI_SYNC, req.agent_run_id)
            .await
            .unwrap();
        ws = Workspace::find_by_id(&pool, ws.id).await.unwrap().unwrap();
        state.active_run_id = Some(req.agent_run_id);
        let lock = store.try_lock().unwrap();
        store.save_state(&state).unwrap();
        drop(lock);
        validate_agent_owner(&pool, &ws, &req, true).await.unwrap();
        let mut spoof = req.clone();
        spoof.session_id = Uuid::new_v4();
        assert!(
            validate_agent_owner(&pool, &ws, &spoof, true)
                .await
                .is_err()
        );
        Workspace::save_execution_result(
            &pool,
            ws.id,
            OPENWIKI_SYNC,
            Some(req.agent_run_id),
            &WorkspaceExecutionResult {
                status: WorkspaceExecutionTerminalStatus::Succeeded,
                completed_at: chrono::Utc::now(),
                error: None,
                wiki_commit: None,
                no_op: true,
            },
        )
        .await
        .unwrap();
        ws = Workspace::find_by_id(&pool, ws.id).await.unwrap().unwrap();
        assert!(ws.is_execution_only());
        assert!(validate_agent_owner(&pool, &ws, &req, true).await.is_err());
    }

    #[tokio::test]
    async fn backfill_uses_evidence_not_names_and_does_not_reclassify_reserved_sources() {
        let (pool, repo, project) = fixture().await;
        let source = workspace(&pool, &repo, None).await;
        let internal = workspace(&pool, &repo, None).await;
        let run = Uuid::new_v4();
        integration(&pool, &repo, project, internal.id, run, None, "succeeded").await;
        sqlx::query("INSERT INTO integration_reservations VALUES('workspace',?,?)")
            .bind(db::models::integration::resource_key(source.id))
            .bind(run)
            .execute(&pool)
            .await
            .unwrap();
        let future = workspace(
            &pool,
            &repo,
            Some(&WorkspaceExecutionOwner::new("future", repo.id, None)),
        )
        .await;
        integration(
            &pool,
            &repo,
            project,
            future.id,
            Uuid::new_v4(),
            None,
            "failed",
        )
        .await;
        backfill(&pool).await.unwrap();
        backfill(&pool).await.unwrap();
        assert!(
            !Workspace::find_by_id(&pool, source.id)
                .await
                .unwrap()
                .unwrap()
                .is_execution_only()
        );
        let restored = Workspace::find_by_id(&pool, internal.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.execution_owner.unwrap().run_id, Some(run));
        assert_eq!(
            Workspace::find_by_id(&pool, future.id)
                .await
                .unwrap()
                .unwrap()
                .execution_owner
                .unwrap()
                .kind,
            "future"
        );
        assert!(cleanup_allowed(&pool, internal.id).await.unwrap());
        assert!(!cleanup_allowed(&pool, future.id).await.unwrap());
    }

    #[tokio::test]
    async fn conflicting_historical_owners_are_inspection_only() {
        let (pool, repo, project) = fixture().await;
        let ws = workspace(&pool, &repo, None).await;
        integration(&pool, &repo, project, ws.id, Uuid::new_v4(), None, "failed").await;
        integration(
            &pool,
            &repo,
            project,
            ws.id,
            Uuid::new_v4(),
            None,
            "succeeded",
        )
        .await;
        backfill(&pool).await.unwrap();
        let ws = Workspace::find_by_id(&pool, ws.id).await.unwrap().unwrap();
        assert!(ws.is_execution_only());
        assert_eq!(
            ws.execution_owner.as_ref().unwrap().kind,
            "conflicting_evidence"
        );
        assert!(
            validate_agent_owner(
                &pool,
                &ws,
                &request(&ws, Uuid::new_v4(), Uuid::new_v4()),
                true
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn owner_dispatch_requires_matching_live_run_session_and_reservation() {
        let (pool, repo, project) = fixture().await;
        let id = Uuid::new_v4();
        let ws = workspace(
            &pool,
            &repo,
            Some(&WorkspaceExecutionOwner::new(
                INTEGRATION,
                repo.id,
                Some(id),
            )),
        )
        .await;
        let session = Session::create(
            &pool,
            &CreateSession {
                name: None,
                executor: None,
            },
            Uuid::new_v4(),
            ws.id,
        )
        .await
        .unwrap();
        integration(
            &pool,
            &repo,
            project,
            ws.id,
            id,
            Some(session.id),
            "integrating",
        )
        .await;
        let req = request(&ws, session.id, id);
        assert!(validate_agent_owner(&pool, &ws, &req, true).await.is_err());
        sqlx::query("INSERT INTO integration_reservations VALUES('workspace',?,?)")
            .bind(db::models::integration::resource_key(ws.id))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        validate_agent_owner(&pool, &ws, &req, true).await.unwrap();
        let mut spoof = req.clone();
        spoof.session_id = Uuid::new_v4();
        assert!(
            validate_agent_owner(&pool, &ws, &spoof, true)
                .await
                .is_err()
        );
        spoof = req.clone();
        spoof.correlation_id = Uuid::new_v4();
        assert!(
            validate_agent_owner(&pool, &ws, &spoof, true)
                .await
                .is_err()
        );
        assert!(!cleanup_allowed(&pool, ws.id).await.unwrap());
        sqlx::query("UPDATE integration_runs SET status='succeeded' WHERE id=?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(validate_agent_owner(&pool, &ws, &req, true).await.is_err());
    }

    #[tokio::test]
    async fn owner_script_is_limited_to_saved_validation_plan_and_passive_inspection_never_creates_files()
     {
        let (pool, repo, project) = fixture().await;
        let id = Uuid::new_v4();
        let mut ws = workspace(
            &pool,
            &repo,
            Some(&WorkspaceExecutionOwner::new(
                INTEGRATION,
                repo.id,
                Some(id),
            )),
        )
        .await;
        let session = Session::create(
            &pool,
            &CreateSession {
                name: None,
                executor: None,
            },
            Uuid::new_v4(),
            ws.id,
        )
        .await
        .unwrap();
        integration(
            &pool,
            &repo,
            project,
            ws.id,
            id,
            Some(session.id),
            "validating",
        )
        .await;
        sqlx::query("INSERT INTO integration_reservations VALUES('workspace',?,?)")
            .bind(db::models::integration::resource_key(ws.id))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let action = ExecutorAction::new(
            ExecutorActionType::ScriptRequest(ScriptRequest {
                script: "test command".into(),
                language: ScriptRequestLanguage::Bash,
                context: ScriptContext::IntegrationValidation,
                working_dir: Some(format!("{}/.", repo.name)),
            }),
            None,
        );
        let reason =
            db::models::execution_process::ExecutionProcessRunReason::IntegrationValidation;
        assert!(
            validate_script_owner(&pool, ws.id, session.id, &action, &reason)
                .await
                .is_err()
        );
        let mut run = db::models::integration::IntegrationRun::find(&pool, id)
            .await
            .unwrap();
        run.payload
            .validation
            .push(db::models::integration::IntegrationValidation {
                command: "test command".into(),
                cwd: ".".into(),
                required: true,
                evidence: "test".into(),
                environment_requirements: String::new(),
                execution_process_id: None,
                exit_code: None,
                result: None,
            });
        run.save(&pool).await.unwrap();
        validate_script_owner(&pool, ws.id, session.id, &action, &reason)
            .await
            .unwrap();
        assert!(
            validate_script_owner(
                &pool,
                ws.id,
                session.id,
                &action,
                &db::models::execution_process::ExecutionProcessRunReason::DevServer
            )
            .await
            .is_err()
        );
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        ws.container_ref = Some(missing.to_string_lossy().into_owned());
        assert!(inspection_root(&ws).is_err());
        assert!(!missing.exists());
        ws.container_ref = Some(temp.path().to_string_lossy().into_owned());
        assert_eq!(inspection_root(&ws).unwrap(), temp.path());
        ws.worktree_deleted = true;
        assert!(inspection_root(&ws).is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM execution_processes")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }
}
