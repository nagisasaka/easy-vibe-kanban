//! Repository maintenance adapter for the existing Workflow runner.
//! The existing repository recovery monitor owns transitions under its OS lock;
//! ordinary Workflow polling/watchers must not independently drive this run.
use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, bail, ensure};
use async_trait::async_trait;
use db::models::{
    agent_runtime::AgentRunRecord, repo::Repo, workflow::WorkflowRunStatus, workspace::Workspace,
};
use deployment::Deployment;
use executors::{
    actions::SelectedSkill,
    runtime::{AgentRunStatus, OrchestrationNodeStatus},
};
use services::services::{
    openwiki::{
        self, OpenWikiAdapter,
        bootstrap::{
            CoverageReview, CoverageVerdict, RefinementReport,
            reports::{self, ReportIdentity, ReportReference},
        },
        completion::WriterPhase,
        inventory::{DocumentInventory, InventoryIdentity},
    },
    orchestration::OrchestrationService,
};
use utils::repository_memory::{
    BootstrapReportKind, OpenWikiBootstrapChild, OpenWikiBootstrapOwner, OpenWikiBootstrapPhase,
    RepositoryMemoryState, RepositoryMemoryStore, RepositoryWikiStatus,
};
use uuid::Uuid;

use super::{
    arena::NoopWorkflowArenaCreator,
    runner::{
        self, AgentNodeExecution, AgentNodeRequest, DeploymentAgentRunReconciliationBoundary,
        DeploymentWorkflowAgentExecutor, WorkflowAgentExecutor,
    },
};
use crate::{DeploymentImpl, error::ApiError};

static SERVER_INSTANCE: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4);

fn api(error: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(error.to_string())
}

struct BootstrapExecutor {
    deployment: DeploymentImpl,
}

#[async_trait]
impl WorkflowAgentExecutor for BootstrapExecutor {
    fn owns_repository_execution(&self) -> bool {
        true
    }
    async fn run_agent(&self, request: AgentNodeRequest) -> Result<AgentNodeExecution, ApiError> {
        DeploymentWorkflowAgentExecutor::new(self.deployment.clone())
            .run_agent(request)
            .await
    }
    async fn publish_repository(&self, run_id: Uuid) -> Result<Option<String>, ApiError> {
        match publish(&self.deployment, run_id).await {
            Ok(result) => Ok(Some(
                serde_json::json!({"wiki_commit": result.0, "no_op": result.1}).to_string(),
            )),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
            {
                Ok(None)
            }
            Err(error) => {
                let ctx = context(&self.deployment, run_id).await.map_err(api)?;
                if ctx.store.publication(run_id).map_err(api)?.is_some() {
                    // Git may already have committed or integrated. Keep the
                    // existing End node pending for publication-only recovery,
                    // never mark it failed and strand a durable checkpoint.
                    let mut state = ctx.store.state().map_err(api)?;
                    state.error = Some(format!("{error:#}"));
                    ctx.store.save_state(&state).map_err(api)?;
                    Ok(None)
                } else {
                    Err(api(error))
                }
            }
        }
    }
}

/// Ownership is durable before the runner can enqueue any child command.
pub async fn start(
    deployment: &DeploymentImpl,
    repo: &Repo,
    store: &RepositoryMemoryStore,
    state: &mut RepositoryMemoryState,
    workspace: &Workspace,
    root: &std::path::Path,
) -> anyhow::Result<()> {
    let run_id = Uuid::new_v4();
    state.status = RepositoryWikiStatus::Initializing;
    state.maintenance_workspace_id = Some(workspace.id);
    state.maintenance_session_id = None;
    state.active_run_id = None;
    state.bootstrap = Some(OpenWikiBootstrapOwner {
        workflow_run_id: run_id,
        server_instance_id: *SERVER_INSTANCE,
        phase: OpenWikiBootstrapPhase::Generating,
        child: None,
        review_fingerprint: None,
    });
    store.save_state(state)?;
    let identity = InventoryIdentity::new(
        repo.id,
        workspace.id,
        run_id,
        state
            .active_source_commit
            .clone()
            .context("Bootstrap source is missing")?,
    );
    let inventory = {
        let root = root.to_path_buf();
        let store = store.clone();
        tokio::task::spawn_blocking(move || {
            DocumentInventory::generate(&git::GitService::new(), &root, &store, identity)
        })
        .await??
    };
    tracing::info!(workflow_run_id = %run_id, inventory = ?inventory, "Bootstrap document inventory prepared");
    let mut graph = workflow::templates::openwiki_bootstrap().graph;
    for node in &mut graph.nodes {
        node.data.prompt_template = match node.id.as_str() {
            "generate" => Some(
                openwiki::bootstrap::writer_prompt(root, &state.output_language, None)
                    + &inventory.prompt(store, false),
            ),
            "review" => Some(
                openwiki::bootstrap::review_prompt(root, &state.output_language)
                    + &inventory.prompt(store, true),
            ),
            _ => node.data.prompt_template.take(),
        };
    }
    runner::reserve_repository_workflow(
        &deployment.db().pool,
        run_id,
        repo.id,
        workspace.id,
        graph,
        &serde_json::to_string(&inventory)?,
    )
    .await?;
    let executor = BootstrapExecutor {
        deployment: deployment.clone(),
    };
    runner::drive_repository_workflow(&deployment.db().pool, run_id, &executor).await?;
    // Dispatch updates the shared owner. Never overwrite its child reservation
    // with the pre-dispatch copy held by the Initialize route.
    *state = store.state()?;
    Ok(())
}

struct ContextData {
    repo: Repo,
    store: RepositoryMemoryStore,
    state: RepositoryMemoryState,
    workspace: Workspace,
    root: PathBuf,
}

/// Workflow input is host-owned. Never recover a reference/digest from agent output
/// or the writable shared manifest. Empty input belongs to pre-inventory runs only.
async fn validated_inventory(
    deployment: &DeploymentImpl,
    ctx: &ContextData,
    run_id: Uuid,
) -> anyhow::Result<Option<DocumentInventory>> {
    let expected = InventoryIdentity::new(
        ctx.repo.id,
        ctx.workspace.id,
        run_id,
        ctx.state
            .active_source_commit
            .clone()
            .context("Bootstrap source missing")?,
    );
    load_inventory_input(&deployment.db().pool, &ctx.store, expected).await
}

pub(super) async fn load_inventory_input(
    pool: &sqlx::SqlitePool,
    store: &RepositoryMemoryStore,
    expected: InventoryIdentity,
) -> anyhow::Result<Option<DocumentInventory>> {
    let input: String = sqlx::query_scalar("SELECT input_text FROM workflow_runs WHERE id = ? AND repository_id = ? AND workspace_id = ? AND issue_id IS NULL AND trigger_source = 'openwiki_bootstrap'")
        .bind(expected.run_id).bind(expected.repository_id).bind(expected.workspace_id).fetch_one(pool).await?;
    if input.is_empty() {
        return Ok(None);
    }
    let inventory: DocumentInventory =
        serde_json::from_str(&input).context("Invalid host inventory input")?;
    let store = store.clone();
    tokio::task::spawn_blocking(move || {
        inventory.validate(&store, &expected)?;
        Ok::<_, anyhow::Error>(Some(inventory))
    })
    .await?
}

async fn context(deployment: &DeploymentImpl, run_id: Uuid) -> anyhow::Result<ContextData> {
    let (repo_id, workspace_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT repository_id, workspace_id FROM workflow_runs WHERE id = ? AND issue_id IS NULL AND workflow_id = ?",
    ).bind(run_id).bind(Uuid::parse_str(workflow::templates::OPENWIKI_BOOTSTRAP_ID)?).fetch_one(&deployment.db().pool).await?;
    let repo = Repo::find_by_id(&deployment.db().pool, repo_id)
        .await?
        .context("Bootstrap repository missing")?;
    let store = RepositoryMemoryStore::for_repository(&repo.name, repo_id)?;
    let state = store.state()?;
    ensure!(
        state
            .bootstrap
            .as_ref()
            .is_some_and(|owner| owner.workflow_run_id == run_id),
        "Bootstrap no longer owns repository maintenance"
    );
    ensure!(
        state.maintenance_workspace_id == Some(workspace_id),
        "Bootstrap workspace ownership mismatch"
    );
    let workspace = Workspace::find_by_id(&deployment.db().pool, workspace_id)
        .await?
        .context("Bootstrap workspace missing")?;
    let root = PathBuf::from(
        workspace
            .container_ref
            .as_ref()
            .context("Bootstrap worktree is unavailable")?,
    )
    .join(&repo.name);
    Ok(ContextData {
        repo,
        store,
        state,
        workspace,
        root,
    })
}

/// Called after stable AgentRun identities are calculated but before outbox
/// dispatch. Ordinary Workflow nodes pass through unchanged.
pub(super) async fn prepare_child_dispatch(
    deployment: &DeploymentImpl,
    request: &mut AgentNodeRequest,
    session_id: Uuid,
    agent_run_id: Uuid,
) -> anyhow::Result<()> {
    let repository_id: Option<Uuid> =
        sqlx::query_scalar("SELECT repository_id FROM workflow_runs WHERE id = ?")
            .bind(request.run_id)
            .fetch_optional(&deployment.db().pool)
            .await?
            .flatten();
    if repository_id.is_none() {
        return Ok(());
    }
    let mut ctx = context(deployment, request.run_id).await?;
    let inventory = validated_inventory(deployment, &ctx, request.run_id).await?;
    let owner = ctx
        .state
        .bootstrap
        .as_ref()
        .context("Missing Bootstrap owner")?;
    ensure!(
        owner.server_instance_id == *SERVER_INSTANCE
            && owner.phase != OpenWikiBootstrapPhase::CleaningUp,
        "Interrupted Bootstrap cannot resume a paid phase"
    );
    ensure!(
        request.iteration == 0 && request.workspace_id == ctx.workspace.id,
        "Bootstrap only supports one fresh execution per phase"
    );
    let source = ctx
        .state
        .active_source_commit
        .as_deref()
        .context("Bootstrap source is missing")?;
    ensure!(
        deployment.git().get_head_info(&ctx.root)?.oid == source,
        "Bootstrap worktree HEAD changed before dispatch"
    );
    // A normal new phase never reuses a Session with prior AgentRuns.
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(&deployment.db().pool)
        .await?;
    ensure!(
        existing == 0,
        "Bootstrap requires a fresh Session; start a new Bootstrap run"
    );
    let phase = match request.node_id.as_str() {
        "generate" => {
            openwiki::setup::prepare(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
            request.prompt =
                openwiki::bootstrap::writer_prompt(&ctx.root, &ctx.state.output_language, None);
            OpenWikiBootstrapPhase::Generating
        }
        "review" => {
            openwiki::setup::restore(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
            request.prompt =
                openwiki::bootstrap::review_prompt(&ctx.root, &ctx.state.output_language);
            ctx.state.bootstrap.as_mut().unwrap().review_fingerprint =
                Some(openwiki::bootstrap::worktree_fingerprint(&ctx.root)?);
            OpenWikiBootstrapPhase::Reviewing
        }
        "refine" => {
            let (review, reference) = validated_review(deployment, &ctx, request.run_id).await?;
            ensure!(
                review.verdict == CoverageVerdict::NeedsRefinement,
                "Refine requires material coverage findings"
            );
            openwiki::setup::prepare_next_phase(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
            request.prompt = openwiki::bootstrap::writer_prompt(
                &ctx.root,
                &ctx.state.output_language,
                Some(&reference.prompt_path(&ctx.store)),
            );
            OpenWikiBootstrapPhase::Refining
        }
        _ => bail!("Unsupported Bootstrap agent phase"),
    };
    if let Some(inventory) = &inventory {
        match phase {
            OpenWikiBootstrapPhase::Generating => request
                .prompt
                .push_str(&inventory.prompt(&ctx.store, false)),
            OpenWikiBootstrapPhase::Reviewing => {
                request.prompt.push_str(&inventory.prompt(&ctx.store, true))
            }
            _ => {} // Refine receives findings, not a new inventory-reading task.
        }
    }
    request.selected_skills = if phase == OpenWikiBootstrapPhase::Reviewing {
        None
    } else {
        Some(vec![SelectedSkill {
            name: "openwiki".into(),
            path: OpenWikiAdapter::installed_skill_path(
                &ctx.root,
                OpenWikiAdapter::requires_project_scope(),
            )?,
        }])
    };
    ctx.state.active_run_id = Some(agent_run_id);
    ctx.state.maintenance_session_id = Some(session_id);
    let owner = ctx.state.bootstrap.as_mut().unwrap();
    owner.phase = phase;
    owner.child = Some(OpenWikiBootstrapChild {
        session_id,
        agent_run_id,
        node_execution_id: request.orchestration_node_execution_id,
        node_id: request.node_id.clone(),
    });
    ctx.store.save_state(&ctx.state)?;
    Ok(())
}

async fn validated_review(
    deployment: &DeploymentImpl,
    ctx: &ContextData,
    run_id: Uuid,
) -> anyhow::Result<(CoverageReview, ReportReference)> {
    load_review_phase(
        &deployment.db().pool,
        &ctx.store,
        source_identity(ctx, run_id)?,
    )
    .await
}

pub(super) async fn load_review_phase(
    pool: &sqlx::SqlitePool,
    store: &RepositoryMemoryStore,
    source: InventoryIdentity,
) -> anyhow::Result<(CoverageReview, ReportReference)> {
    let (raw, identity) = phase_report_input(pool, source, BootstrapReportKind::Review).await?;
    if reports::is_reference(&raw) {
        let reference = ReportReference::parse(&raw)?;
        let review = reference.load_review(store, &identity)?;
        Ok((review, reference))
    } else {
        // Existing inline executions remain readable. The DB's full JSON is the
        // authority; materialise a file only to support reference-based dispatch.
        let review = CoverageReview::parse(&raw)?;
        let reference = reports::save_review(store, identity, &review)?;
        Ok((review, reference))
    }
}

fn source_identity(ctx: &ContextData, run_id: Uuid) -> anyhow::Result<InventoryIdentity> {
    Ok(InventoryIdentity::new(
        ctx.repo.id,
        ctx.workspace.id,
        run_id,
        ctx.state
            .active_source_commit
            .clone()
            .context("Bootstrap source missing")?,
    ))
}

/// Bind the file to the successful phase's DB identities, not fields supplied
/// by the file itself or by the currently active (possibly different) child.
pub(super) async fn phase_report_input(
    pool: &sqlx::SqlitePool,
    source: InventoryIdentity,
    phase: BootstrapReportKind,
) -> anyhow::Result<(String, ReportIdentity)> {
    let node = match phase {
        BootstrapReportKind::Review => "review",
        BootstrapReportKind::Refine => "refine",
    };
    let (raw, session_id, agent_run_id): (String, Uuid, Uuid) = sqlx::query_as(
        "SELECT output_text, session_id, agent_run_id FROM node_executions WHERE run_id = ? AND node_id = ? AND iteration = 0 AND status = 'succeeded'")
        .bind(source.run_id).bind(node).fetch_one(pool).await?;
    Ok((
        raw,
        ReportIdentity {
            source,
            phase,
            session_id,
            agent_run_id,
        },
    ))
}

pub(super) async fn validate_publication_reports(
    pool: &sqlx::SqlitePool,
    store: &RepositoryMemoryStore,
    source: InventoryIdentity,
    root: &std::path::Path,
) -> anyhow::Result<()> {
    let (review, _) = load_review_phase(pool, store, source.clone()).await?;
    if review.verdict == CoverageVerdict::NeedsRefinement {
        let (raw, identity) = phase_report_input(pool, source, BootstrapReportKind::Refine).await?;
        let report = if reports::is_reference(&raw) {
            ReportReference::parse(&raw)?.load_refinement(store, &identity, &review)?
        } else {
            RefinementReport::parse(&raw, &review)?
        };
        report.validate_files(root)?;
    }
    Ok(())
}

pub(super) async fn validate_child_completion(
    deployment: &DeploymentImpl,
    run_id: Uuid,
    node_id: &str,
    agent_run_id: Uuid,
    output: &str,
) -> anyhow::Result<String> {
    let ctx = context(deployment, run_id).await?;
    validated_inventory(deployment, &ctx, run_id).await?;
    let owner = ctx.state.bootstrap.as_ref().unwrap();
    ensure!(
        owner
            .child
            .as_ref()
            .is_some_and(|child| child.agent_run_id == agent_run_id && child.node_id == node_id),
        "Completion is not from the delegated Bootstrap child"
    );
    ensure!(
        owner.phase != OpenWikiBootstrapPhase::CleaningUp,
        "Bootstrap is cleaning up, not accepting successful completion"
    );
    let source = ctx
        .state
        .active_source_commit
        .as_deref()
        .context("Bootstrap source missing")?;
    ensure!(
        deployment.git().get_head_info(&ctx.root)?.oid == source,
        "Bootstrap agent changed Git history"
    );
    let run = AgentRunRecord::find(&deployment.db().pool, agent_run_id)
        .await?
        .context("Bootstrap AgentRun missing")?;
    ensure!(
        owner
            .child
            .as_ref()
            .is_some_and(|child| child.session_id == run.session_id),
        "Bootstrap child Session mismatch"
    );
    ensure!(
        run.status == AgentRunStatus::Succeeded,
        "Bootstrap child did not succeed"
    );
    let report_identity = |phase| -> anyhow::Result<ReportIdentity> {
        Ok(ReportIdentity {
            source: source_identity(&ctx, run_id)?,
            phase,
            session_id: run.session_id,
            agent_run_id,
        })
    };
    if node_id == "review" {
        ensure!(
            owner.review_fingerprint.as_deref()
                == Some(&openwiki::bootstrap::worktree_fingerprint(&ctx.root)?),
            "Read-only reviewer changed the repository"
        );
        let review = CoverageReview::parse(output)?;
        for finding in &review.findings {
            for path in &finding.evidence_paths {
                utils::repository_memory::reject_symlinks(&ctx.root.join(path))?;
                ensure!(
                    ctx.root.join(path).is_file(),
                    "Review evidence file does not exist: {path}"
                );
            }
        }
        let reference = reports::save_review(
            &ctx.store,
            report_identity(BootstrapReportKind::Review)?,
            &review,
        )?;
        return Ok(serde_json::to_string(&reference)?);
    }
    ensure!(
        matches!(node_id, "generate" | "refine"),
        "Unexpected writer phase"
    );
    let review = if node_id == "refine" {
        Some(validated_review(deployment, &ctx, run_id).await?.0)
    } else {
        None
    };
    let report = if node_id == "refine" {
        Some(RefinementReport::parse(
            output,
            review.as_ref().context("Refine review missing")?,
        )?)
    } else {
        None
    };
    let proof = crate::routes::openwiki::completion::writer_completion_proof(
        &deployment.db().pool,
        &run,
        ctx.workspace.id,
        std::path::Path::new(
            ctx.workspace
                .container_ref
                .as_deref()
                .context("Bootstrap workspace path missing")?,
        ),
        &ctx.root,
        if node_id == "refine" {
            WriterPhase::Refine
        } else {
            WriterPhase::Generate
        },
    )
    .await?;
    if let Some(report) = &report {
        ensure!(
            !report.requires_update() || proof.has_completed_update(),
            "Refine claimed fixed findings without a completed update"
        );
    }
    openwiki::setup::validate_provenance(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
    openwiki::setup::restore(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
    openwiki::setup::validate_provenance(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
    openwiki::publication_paths(deployment.git(), &ctx.root, source)?;
    if let Some(report) = report {
        report.validate_files(&ctx.root)?;
        if !proof.has_completed_update() {
            ensure!(
                owner.review_fingerprint.as_deref()
                    == Some(&openwiki::bootstrap::worktree_fingerprint(&ctx.root)?),
                "Refine changed files without a verified OpenWiki update"
            );
        }
        let reference = reports::save_refinement(
            &ctx.store,
            report_identity(BootstrapReportKind::Refine)?,
            &report,
            review.as_ref().context("Refine review missing")?,
        )?;
        return Ok(serde_json::to_string(&reference)?);
    }
    // No generator narrative is needed by the fresh reviewer or router.
    Ok(format!(
        "OpenWiki {node_id} completed; Native Audit and source invariants verified"
    ))
}

async fn publish(
    deployment: &DeploymentImpl,
    run_id: Uuid,
) -> anyhow::Result<(Option<String>, bool)> {
    let mut ctx = context(deployment, run_id).await?;
    validated_inventory(deployment, &ctx, run_id).await?;
    let run = runner::get_workflow_run_response(&deployment.db().pool, run_id).await?;
    for node in ["generate", "review"] {
        ensure!(
            run.nodes.iter().any(|item| item.node_id == node
                && item.status == db::models::workflow::NodeExecutionStatus::Succeeded),
            "Bootstrap has not validated {node}"
        );
    }
    validate_publication_reports(
        &deployment.db().pool,
        &ctx.store,
        source_identity(&ctx, run_id)?,
        &ctx.root,
    )
    .await?;
    ensure!(
        ctx.state.bootstrap.as_ref().unwrap().phase != OpenWikiBootstrapPhase::CleaningUp,
        "Failed Bootstrap cannot publish"
    );
    let source = ctx
        .state
        .active_source_commit
        .as_deref()
        .context("Bootstrap source missing")?;
    if ctx.store.publication(run_id)?.is_none() {
        openwiki::setup::restore(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
        openwiki::setup::validate_provenance(&ctx.store, ctx.workspace.id, &ctx.root, source)?;
    }
    ctx.state.bootstrap.as_mut().unwrap().phase = OpenWikiBootstrapPhase::Publishing;
    ctx.store.save_state(&ctx.state)?;
    openwiki::publish_validated_wiki(
        deployment.git(),
        &ctx.store,
        &openwiki::WikiPublicationRequest {
            repository_root: &ctx.repo.path,
            maintenance_root: &ctx.root,
            maintenance_branch: &ctx.workspace.branch,
            target_branch: ctx
                .state
                .target_branch
                .as_deref()
                .context("Bootstrap target missing")?,
            source_commit: source,
            run_id,
        },
    )
}

/// Use the existing repository monitor under its repository lock. Ordinary
/// Workflow event watchers never independently advance repository-owned runs.
pub async fn recover_locked(
    deployment: &DeploymentImpl,
    repo: &Repo,
    store: &RepositoryMemoryStore,
) -> anyhow::Result<()> {
    let mut state = store.state()?;
    let owner = state.bootstrap.clone().context("No Bootstrap owner")?;
    let run_id = owner.workflow_run_id;
    if owner.server_instance_id != *SERVER_INSTANCE && store.publication(run_id)?.is_none() {
        state.error = Some("Bootstrap interrupted by a server restart; start a new Initialise Wiki request after cleanup".into());
        state.bootstrap.as_mut().unwrap().phase = OpenWikiBootstrapPhase::CleaningUp;
        store.save_state(&state)?;
    }
    if state.bootstrap.as_ref().unwrap().phase == OpenWikiBootstrapPhase::CleaningUp {
        return cleanup(deployment, repo, store).await;
    }
    let executor = BootstrapExecutor {
        deployment: deployment.clone(),
    };
    let boundary = DeploymentAgentRunReconciliationBoundary::for_repository(deployment.clone());
    let result = async {
        let run = runner::reconcile_workflow_run_with_arena_and_boundary(
            &deployment.db().pool,
            run_id,
            &executor,
            &NoopWorkflowArenaCreator,
            &boundary,
        )
        .await?;
        if matches!(
            run.status,
            WorkflowRunStatus::Failed | WorkflowRunStatus::Canceled | WorkflowRunStatus::Cancelling
        ) {
            bail!(
                "{}",
                run.nodes
                    .iter()
                    .find_map(|node| node.error_text.as_deref())
                    .or(run.error_text.as_deref())
                    .unwrap_or("Bootstrap failed or was cancelled")
            );
        }
        // A deferred host publication can leave a ready node without a new
        // AgentRun terminal event to wake it. The same planner handles it.
        runner::drive_repository_workflow(&deployment.db().pool, run_id, &executor).await?;
        sync_product_nodes(deployment, run_id).await?;
        let run = runner::get_workflow_run_response(&deployment.db().pool, run_id).await?;
        if matches!(
            run.status,
            WorkflowRunStatus::Failed | WorkflowRunStatus::Canceled
        ) {
            bail!(
                "{}",
                run.nodes
                    .iter()
                    .find_map(|node| node.error_text.as_deref())
                    .unwrap_or("Bootstrap failed")
            );
        }
        if run.status == WorkflowRunStatus::Succeeded {
            let publication = publish(deployment, run_id).await?;
            let mut latest = store.state()?;
            crate::routes::openwiki::record_reconciliation_result(
                store,
                &mut latest,
                Ok(publication),
            )?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(error) = result {
        let mut latest = store.state()?;
        latest.error = Some(format!("{error:#}"));
        // Publication recovery is Git/receipt-only. Never restart paid hosts
        // or discard the identity of a possibly integrated Wiki.
        if store.publication(run_id)?.is_some() {
            store.save_state(&latest)?;
            return Err(error);
        }
        latest
            .bootstrap
            .as_mut()
            .context("Bootstrap owner disappeared")?
            .phase = OpenWikiBootstrapPhase::CleaningUp;
        store.save_state(&latest)?;
        cleanup(deployment, repo, store).await?;
    }
    Ok(())
}

async fn sync_product_nodes(deployment: &DeploymentImpl, run_id: Uuid) -> anyhow::Result<()> {
    let run = runner::get_workflow_run_response(&deployment.db().pool, run_id).await?;
    let service = OrchestrationService::new(
        deployment.db().pool.clone(),
        Arc::new(deployment.agent_run_port().clone()),
    );
    let orchestration_id = run
        .orchestration_run_id
        .context("Bootstrap orchestration identity missing")?;
    for node in &run.nodes {
        let status = match node.status {
            db::models::workflow::NodeExecutionStatus::Succeeded => {
                OrchestrationNodeStatus::Succeeded
            }
            db::models::workflow::NodeExecutionStatus::Skipped => {
                OrchestrationNodeStatus::Cancelled
            }
            _ => continue,
        };
        if let Some(id) = node.orchestration_node_execution_id {
            service
                .complete_product_node(orchestration_id, id, status)
                .await?;
        }
    }
    service
        .refresh_run_projection(orchestration_id, orchestration_id)
        .await?;
    Ok(())
}

async fn cleanup(
    deployment: &DeploymentImpl,
    repo: &Repo,
    store: &RepositoryMemoryStore,
) -> anyhow::Result<()> {
    let pool = &deployment.db().pool;
    let service =
        OrchestrationService::new(pool.clone(), Arc::new(deployment.agent_run_port().clone()));
    cleanup_with_service(pool, repo, store, &service).await
}

/// Keep cleanup on the same durable orchestration service boundary as normal
/// control, allowing failure/lease tests without launching a provider.
pub(super) async fn cleanup_with_service<P: executors::runtime::AgentRunPort + 'static>(
    pool: &sqlx::SqlitePool,
    repo: &Repo,
    store: &RepositoryMemoryStore,
    service: &OrchestrationService<P>,
) -> anyhow::Result<()> {
    use executors::runtime::{
        AgentRunPortCommand, AgentRunPortCommandEnvelope, ORCHESTRATION_COMMAND_SCHEMA_VERSION,
    };
    let mut state = store.state()?;
    let owner = state
        .bootstrap
        .clone()
        .context("Bootstrap owner missing during cleanup")?;
    let orchestration_id: Option<Uuid> =
        sqlx::query_scalar("SELECT orchestration_run_id FROM workflow_runs WHERE id = ?")
            .bind(owner.workflow_run_id)
            .fetch_optional(pool)
            .await?
            .flatten();
    if let Some(id) = orchestration_id {
        service.cancel(id, id).await?;
    }
    // Cover a crash after host creation but before its orchestration link.
    if let Some(child) = &owner.child
        && let Some(run) = AgentRunRecord::find(pool, child.agent_run_id).await?
        && !run.status.is_terminal()
    {
        let id = orchestration_id
            .context("Active Bootstrap child has no orchestration identity; keep fenced")?;
        service
            .enqueue_command(AgentRunPortCommandEnvelope {
                schema_version: ORCHESTRATION_COMMAND_SCHEMA_VERSION,
                command_id: runner::stable_workflow_identity(
                    id,
                    child.node_execution_id,
                    0,
                    "bootstrap-cleanup",
                ),
                idempotency_key: format!(
                    "bootstrap:{}:cleanup:{}",
                    owner.workflow_run_id, child.agent_run_id
                ),
                agent_run_id: child.agent_run_id,
                orchestration_run_id: Some(id),
                orchestration_node_execution_id: Some(child.node_execution_id),
                correlation_id: id,
                created_at: run.created_at,
                command: AgentRunPortCommand::Cancel {
                    reason: "OpenWiki Bootstrap cleanup".into(),
                },
            })
            .await?;
    }
    // Bounded delivery of normal audited commands; never an unaudited kill.
    for _ in 0..32 {
        if !service.deliver_next().await? {
            break;
        }
    }
    if let Some(child) = &owner.child
        && AgentRunRecord::find(pool, child.agent_run_id)
            .await?
            .is_some_and(|run| !run.status.is_terminal())
    {
        return Ok(()); // Requested Stop is not proof of process exit.
    }
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs ar JOIN sessions s ON s.id = ar.session_id WHERE s.workspace_id = ? AND ar.status NOT IN ('succeeded','failed','cancelled','crashed','audit_failed')")
        .bind(state.maintenance_workspace_id).fetch_one(pool).await?;
    ensure!(
        active == 0,
        "Bootstrap still has active children; keep maintenance fenced"
    );
    let children: Vec<(Uuid, AgentRunStatus, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT ar.id, ar.status, ar.updated_at FROM agent_runs ar JOIN sessions s ON s.id = ar.session_id WHERE s.workspace_id = ?",
    ).bind(state.maintenance_workspace_id).fetch_all(pool).await?;
    for (id, status, updated_at) in children {
        if let Err(error) = crate::routes::openwiki::completion::ensure_processes_exited(
            pool, id, status, updated_at,
        )
        .await
        {
            if error
                .downcast_ref::<crate::routes::openwiki::completion::CompletionPending>()
                .is_none()
            {
                state.error = Some(format!("Bootstrap cleanup retains ownership: {error:#}"));
                store.save_state(&state)?;
            }
            return Ok(());
        }
    }
    if let Some(workspace_id) = state.maintenance_workspace_id {
        let workspace = Workspace::find_by_id(pool, workspace_id)
            .await?
            .context("Cannot restore missing Bootstrap workspace")?;
        let root = PathBuf::from(
            workspace
                .container_ref
                .context("Cannot restore missing Bootstrap path")?,
        )
        .join(&repo.name);
        openwiki::setup::restore(
            store,
            workspace_id,
            &root,
            state
                .active_source_commit
                .as_deref()
                .context("Missing Bootstrap source")?,
        )?;
    }
    sqlx::query("UPDATE node_executions SET status = CASE WHEN status = 'pending' THEN 'skipped' ELSE 'cancelled' END, finished_at = datetime('now','subsec'), updated_at = datetime('now','subsec') WHERE run_id = ? AND status IN ('pending','running','awaiting_human','cancelling')")
        .bind(owner.workflow_run_id).execute(pool).await?;
    let reason = state
        .error
        .clone()
        .unwrap_or_else(|| "OpenWiki Bootstrap failed or was cancelled".into());
    runner::update_run_status(
        pool,
        owner.workflow_run_id,
        WorkflowRunStatus::Failed,
        None,
        Some(&reason),
        true,
    )
    .await?;
    if let Some(id) = orchestration_id {
        service.reconcile_run(id, id).await?;
    }
    crate::routes::openwiki::record_reconciliation_result(
        store,
        &mut state,
        Err(anyhow::anyhow!(reason)),
    )?;
    Ok(())
}

/// Serialize an explicit Workflow Stop with phase transitions/publication.
pub async fn cancel_owned_run(deployment: &DeploymentImpl, run_id: Uuid) -> Result<(), ApiError> {
    let current = runner::get_workflow_run_response(&deployment.db().pool, run_id).await?;
    if matches!(
        current.status,
        WorkflowRunStatus::Succeeded | WorkflowRunStatus::Failed | WorkflowRunStatus::Canceled
    ) {
        return Ok(());
    }
    let ctx = context(deployment, run_id).await.map_err(api)?;
    let _lock = ctx.store.try_lock().map_err(api)?;
    let mut state = ctx.store.state().map_err(api)?;
    if ctx.store.publication(run_id).map_err(api)?.is_some() {
        return Err(api(
            "Wiki publication has already started; its exact result must be reconciled before releasing ownership",
        ));
    }
    state
        .bootstrap
        .as_mut()
        .ok_or_else(|| api("Bootstrap owner missing"))?
        .phase = OpenWikiBootstrapPhase::CleaningUp;
    state.error = Some("Bootstrap cancelled by the user".into());
    ctx.store.save_state(&state).map_err(api)?;
    cleanup(deployment, &ctx.repo, &ctx.store)
        .await
        .map_err(api)
}

/// Fence before generic startup delivery: old queued Creates must not launch
/// paid phases. Existing hosts receive normal cancellation during cleanup.
pub async fn fence_interrupted_runs(deployment: &DeploymentImpl) -> anyhow::Result<()> {
    for repo in Repo::list_all(&deployment.db().pool).await? {
        let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)?
        else {
            continue;
        };
        let _lock = store.try_lock()?;
        let mut state = store.state()?;
        if let Some(owner) = state.bootstrap.as_mut()
            && owner.server_instance_id != *SERVER_INSTANCE
            && store.publication(owner.workflow_run_id)?.is_none()
        {
            let workflow_run_id = owner.workflow_run_id;
            owner.phase = OpenWikiBootstrapPhase::CleaningUp;
            state.error = Some("Bootstrap interrupted by server restart; cleaning up without resuming agent phases".into());
            store.save_state(&state)?;
            if let Some(id) = sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT orchestration_run_id FROM workflow_runs WHERE id = ?",
            )
            .bind(workflow_run_id)
            .fetch_optional(&deployment.db().pool)
            .await?
            .flatten()
            {
                OrchestrationService::new(
                    deployment.db().pool.clone(),
                    Arc::new(deployment.agent_run_port().clone()),
                )
                .cancel(id, id)
                .await?;
            }
        }
    }
    Ok(())
}
