use std::{collections::HashSet, path::PathBuf};

use anyhow::{Context, ensure};
use db::models::{
    integration::{
        IntegrationPayload, IntegrationRun, IntegrationSelection, IntegrationSource, resource_key,
    },
    repo::Repo,
    workspace::Workspace,
    workspace_repo::WorkspaceRepo,
};
use deployment::Deployment;
use executors::{
    executors::BaseCodingAgent,
    model_selector::PermissionPolicy,
    profile::{ExecutionMode, ExecutorConfig},
};
use services::services::container::ContainerService;
use sqlx::{Row, SqlitePool};
use utils::repository_memory::RepositoryMemoryStore;
use uuid::Uuid;

use super::CreateIntegrationRequest;
use crate::DeploymentImpl;

pub(super) async fn idle(deployment: &DeploymentImpl, workspace: Uuid) -> anyhow::Result<()> {
    let pool = &deployment.db().pool;
    let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_runs WHERE workspace_id=? AND status NOT IN ('succeeded','failed','cancelled','crashed','audit_failed')) OR EXISTS(SELECT 1 FROM execution_processes p JOIN sessions s ON s.id=p.session_id WHERE s.workspace_id=? AND p.status='running') OR EXISTS(SELECT 1 FROM agent_process_registry p JOIN agent_run_attempts a ON a.id=p.run_attempt_id JOIN agent_runs r ON r.id=a.agent_run_id WHERE r.workspace_id=? AND p.registry_status IN ('spawned','running','unreachable'))")
        .bind(workspace).bind(workspace).bind(workspace).fetch_one(pool).await?;
    ensure!(
        !active,
        "Workspace {workspace} has an active or unconfirmed writer"
    );
    ensure!(
        deployment.container().scripts_settled(workspace).await?,
        "Workspace {workspace} has unfinished script finalisation/process cleanup"
    );
    let sessions: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sessions WHERE workspace_id=?")
        .bind(workspace)
        .fetch_all(pool)
        .await?;
    for session in sessions {
        ensure!(
            !deployment.queued_message_service().has_queued(session),
            "Workspace {workspace} has a queued follow-up"
        );
        let goal: Option<Option<String>> = sqlx::query_scalar("SELECT json_extract(st.state_json,'$.goal.status') FROM agent_runs r JOIN agent_run_state st ON st.agent_run_id=r.id WHERE r.session_id=? ORDER BY r.created_at DESC,r.id DESC LIMIT 1")
            .bind(session).fetch_optional(pool).await?;
        ensure!(
            goal.flatten().as_deref() != Some("active"),
            "Workspace {workspace} has an active saved Goal; finish or pause it first"
        );
    }
    Ok(())
}

pub(super) async fn source(
    deployment: &DeploymentImpl,
    request: &CreateIntegrationRequest,
    selected: &IntegrationSelection,
) -> anyhow::Result<IntegrationSource> {
    inspect_source(deployment, request, selected, true).await
}

pub(super) async fn source_for_completion(
    deployment: &DeploymentImpl,
    request: &CreateIntegrationRequest,
    selected: &IntegrationSelection,
) -> anyhow::Result<IntegrationSource> {
    inspect_source(deployment, request, selected, false).await
}

async fn inspect_source(
    deployment: &DeploymentImpl,
    request: &CreateIntegrationRequest,
    selected: &IntegrationSelection,
    inspect_memory: bool,
) -> anyhow::Result<IntegrationSource> {
    let pool = &deployment.db().pool;
    let row=sqlx::query("SELECT i.title,i.description,i.status_id,i.integration_revision FROM local_issues i JOIN local_workspace_links l ON l.issue_id=i.id WHERE i.id=? AND i.project_id=? AND l.workspace_id=?")
        .bind(selected.card_id).bind(request.project_id).bind(selected.workspace_id).fetch_optional(pool).await?.context("Adopted Workspace must be currently linked to the selected local Card/project")?;
    let workspaces: Vec<Uuid> = sqlx::query_scalar(
        "SELECT workspace_id FROM local_workspace_links WHERE issue_id=? ORDER BY workspace_id",
    )
    .bind(selected.card_id)
    .fetch_all(pool)
    .await?;
    let repo = Repo::find_by_id(pool, request.repository_id)
        .await?
        .context("Repository missing")?;
    let mut commit = None;
    let mut branch = None;
    for id in &workspaces {
        idle(deployment, *id).await?;
        let workspace = Workspace::find_by_id(pool, *id)
            .await?
            .context("Workspace missing")?;
        workspace.require_interactive()?;
        let repos = WorkspaceRepo::find_repos_for_workspace(pool, *id).await?;
        for member in repos {
            let membership = WorkspaceRepo::find_by_workspace_and_repo_id(pool, *id, member.id)
                .await?
                .context("Repository membership missing")?;
            let head = deployment
                .git()
                .get_branch_oid(&member.path, &workspace.branch)?;
            let container=workspace.container_ref.as_ref().context("Source Workspace has no checkout; inspect/recreate it before selecting Integration")?;
            let root = if workspace.is_direct_folder() {
                PathBuf::from(container)
            } else {
                PathBuf::from(container).join(&member.name)
            };
            deployment.git().require_clean_source(&root, &head)?;
            if member.id != request.repository_id {
                let target = deployment
                    .git()
                    .get_branch_oid(&member.path, &membership.target_branch)?;
                ensure!(
                    deployment.git().is_ancestor(&member.path, &head, &target)?,
                    "Card {} has unintegrated results in another repository {}",
                    selected.card_id,
                    member.display_name
                );
            }
            if inspect_memory
                && let Some(store) =
                    RepositoryMemoryStore::existing_for_repository(&member.name, member.id)?
            {
                for run in store
                    .runs()?
                    .into_iter()
                    .filter(|run| run.workspace_id == *id && run.finalize_source)
                {
                    let succeeded: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE id=? AND status='succeeded')",
                    )
                    .bind(run.run_id)
                    .fetch_one(pool)
                    .await?;
                    ensure!(
                        !succeeded
                            || store.event(run.run_id)?.is_some()
                            || store.completed_without_change(run.run_id)?,
                        "Source/Manifest finalisation is unfinished for AgentRun {}",
                        run.run_id
                    );
                }
            }
            if *id == selected.workspace_id && member.id == request.repository_id {
                commit = Some(head);
                branch = Some(workspace.branch.clone());
            }
        }
    }
    let commit = commit.context("Selected Workspace does not contain the chosen repository")?;
    ensure!(
        commit == selected.expected_commit,
        "SOURCE_CHANGED: adopted Workspace HEAD changed since selection; explicitly reselect the observed commit"
    );
    let mut event_ids = Vec::new();
    if inspect_memory
        && let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)?
    {
        // Strict durable evidence, separate from the tolerant discovery reader.
        for event in store.events()? {
            if event.repository_id == repo.id
                && event.workspace_id == selected.workspace_id
                && deployment
                    .git()
                    .is_ancestor(&repo.path, &event.source_commit, &commit)?
            {
                event_ids.push(event.event_id);
            }
        }
    }
    event_ids.sort();
    Ok(IntegrationSource {
        selection: selected.clone(),
        branch: branch.context("Source branch missing")?,
        commit,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        status_id: row.try_get("status_id")?,
        requirements_revision: row.try_get("integration_revision")?,
        event_ids,
        related_workspaces: workspaces,
        done_result: None,
    })
}

pub(super) async fn submit(
    deployment: &DeploymentImpl,
    request: CreateIntegrationRequest,
) -> anyhow::Result<IntegrationRun> {
    let _admission = services::services::integration_admission::MUTATIONS
        .lock()
        .await;
    let pool = &deployment.db().pool;
    ensure!(
        !request.selections.is_empty() && request.selections.len() <= 100,
        "Select one or more Cards (request safety bound exceeded)"
    );
    if let Some(config) = &request.executor_config {
        ensure!(
            config.execution_mode.unwrap_or_default() == ExecutionMode::Code
                && config.permission_policy != Some(PermissionPolicy::Plan),
            "Formal Integration requires Code mode; independent Goal/Plan continuations cannot publish a host-validated result"
        );
    }
    let cards: HashSet<_> = request.selections.iter().map(|s| s.card_id).collect();
    let workspaces: HashSet<_> = request.selections.iter().map(|s| s.workspace_id).collect();
    ensure!(
        cards.len() == request.selections.len() && workspaces.len() == cards.len(),
        "Use exactly one adopted Workspace per Card"
    );
    let target = request
        .target_branch
        .strip_prefix("refs/heads/")
        .unwrap_or(&request.target_branch);
    ensure!(
        !target.starts_with("refs/") && !target.starts_with('-'),
        "Choose a local target branch"
    );
    let target_ref = format!("refs/heads/{target}");
    let key = request.request_id.to_string();
    if let Some(run) =
        sqlx::query_as::<_, IntegrationRun>("SELECT * FROM integration_runs WHERE request_key=?")
            .bind(&key)
            .fetch_optional(pool)
            .await?
    {
        ensure!(
            run.project_id == request.project_id
                && run.repository_id == request.repository_id
                && run.target_ref == target_ref
                && run.payload.executor_config
                    == Some(serde_json::to_value(
                        request
                            .executor_config
                            .clone()
                            .unwrap_or_else(|| ExecutorConfig::new(BaseCodingAgent::Codex))
                    )?)
                && run.payload.sources.len() == request.selections.len()
                && run
                    .payload
                    .sources
                    .iter()
                    .zip(&request.selections)
                    .all(|(s, r)| s.selection.card_id == r.card_id
                        && s.selection.workspace_id == r.workspace_id
                        && s.selection.expected_commit == r.expected_commit),
            "Idempotency conflict: explicit selection changed"
        );
        return Ok(run);
    }
    let repo = Repo::find_by_id(pool, request.repository_id)
        .await?
        .context("Repository missing")?;
    let registered = super::project_repository_ids(pool, request.project_id)
        .await?
        .contains(&repo.id);
    ensure!(registered, "Repository must belong to this local project");
    let observed_target = deployment.git().get_branch_oid(&repo.path, target)?;
    let storage = deployment
        .git()
        .storage_identity(&repo.path)?
        .to_string_lossy()
        .into_owned();
    let mut sources = Vec::new();
    for selection in &request.selections {
        sources.push(source(deployment, &request, selection).await?);
    }
    let payload = IntegrationPayload {
        sources,
        observed_target,
        executor_config: Some(serde_json::to_value(
            request
                .executor_config
                .clone()
                .unwrap_or_else(|| ExecutorConfig::new(BaseCodingAgent::Codex)),
        )?),
        ..Default::default()
    };
    let id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO integration_runs(id,request_key,project_id,repository_id,storage_identity,target_ref,status,payload) VALUES(?,?,?,?,?,?,'queued',?)")
        .bind(id).bind(key).bind(request.project_id).bind(repo.id).bind(&storage).bind(&target_ref).bind(sqlx::types::Json(&payload)).execute(&mut *tx).await?;
    for source in &payload.sources {
        sqlx::query("INSERT INTO integration_reservations VALUES('card',?,?)")
            .bind(resource_key(source.selection.card_id))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for ws in &source.related_workspaces {
            sqlx::query("INSERT INTO integration_reservations VALUES('workspace',?,?)")
                .bind(resource_key(*ws))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            // Atomic against Agent and Script admission, including setup-gated
            // pending launches. Check again after obtaining the writer lock.
            let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_runs WHERE workspace_id=? AND status NOT IN ('succeeded','failed','cancelled','crashed','audit_failed')) OR EXISTS(SELECT 1 FROM execution_processes p JOIN sessions s ON s.id=p.session_id WHERE s.workspace_id=? AND p.status='running')")
                .bind(ws).bind(ws).fetch_one(&mut *tx).await?;
            ensure!(
                !active,
                "A source writer started during selection; retry after it finishes"
            );
        }
        let unchanged:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_issues i JOIN local_workspace_links l ON l.issue_id=i.id WHERE i.id=? AND i.title=? AND i.description IS ? AND i.status_id=? AND i.integration_revision=? AND l.workspace_id=?)")
            .bind(source.selection.card_id).bind(&source.title).bind(&source.description).bind(source.status_id).bind(source.requirements_revision).bind(source.selection.workspace_id).fetch_one(&mut *tx).await?;
        ensure!(
            unchanged,
            "Card requirements/link changed during selection; reselect"
        );
    }
    tx.commit().await?;
    // External Git is outside DB isolation; a second full verification after
    // admission closes ordinary UI races and never silently changes frozen OIDs.
    let run = IntegrationRun::find(pool, id).await?;
    if let Err(error) = verify_sources(deployment, &run).await {
        sqlx::query("UPDATE integration_runs SET status='blocked',error=? WHERE id=?")
            .bind(format!("{error:#}"))
            .bind(id)
            .execute(pool)
            .await?;
        release(pool, id).await?;
    }
    Ok(IntegrationRun::find(pool, id).await?)
}

pub(super) async fn verify_sources(
    deployment: &DeploymentImpl,
    run: &IntegrationRun,
) -> anyhow::Result<()> {
    let request = CreateIntegrationRequest {
        request_id: run.id,
        project_id: run.project_id,
        repository_id: run.repository_id,
        target_branch: run.target_ref.clone(),
        selections: vec![],
        executor_config: None,
    };
    for frozen in &run.payload.sources {
        let current = source(deployment, &request, &frozen.selection).await?;
        ensure!(
            current.commit == frozen.commit
                && current.branch == frozen.branch
                && current.title == frozen.title
                && current.description == frozen.description
                && current.status_id == frozen.status_id
                && current.requirements_revision == frozen.requirements_revision
                && current.related_workspaces == frozen.related_workspaces,
            "Selected source/Card changed; frozen selection is no longer valid"
        );
    }
    Ok(())
}

pub(super) async fn release(pool: &SqlitePool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM integration_reservations WHERE run_id=?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(super) async fn target_right(pool: &SqlitePool, run: &IntegrationRun) -> anyhow::Result<bool> {
    // Conservative repository-wide serialisation, keyed by canonical Git
    // storage, even when the repository is registered under another ID.
    let first:Option<Uuid>=sqlx::query_scalar("SELECT id FROM integration_runs WHERE storage_identity=? AND status IN ('queued','preparing','integrating','validating','publishing','post_processing','cancelling','recovery_required') ORDER BY created_at,id LIMIT 1")
        .bind(&run.storage_identity).fetch_optional(pool).await?;
    if first != Some(run.id) {
        return Ok(false);
    }
    sqlx::query("INSERT INTO integration_reservations(resource_kind,resource_key,run_id) VALUES('target',?,?) ON CONFLICT(resource_kind,resource_key) DO NOTHING")
        .bind(&run.storage_identity).bind(run.id).execute(pool).await?;
    let owner:Uuid=sqlx::query_scalar("SELECT run_id FROM integration_reservations WHERE resource_kind='target' AND resource_key=?").bind(&run.storage_identity).fetch_one(pool).await?;
    Ok(owner == run.id)
}
