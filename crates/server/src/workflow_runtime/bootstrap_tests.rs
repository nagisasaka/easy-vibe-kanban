//! No-model tests of the real Workflow planner, repository run reservation,
//! fresh Session bindings, branch skip semantics and existing Git publication.
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use services::services::openwiki::{self, bootstrap::CoverageReview};
use utils::repository_memory::RepositoryMemoryStore;

use super::*;

fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

struct Fixture {
    _temp: tempfile::TempDir,
    pool: SqlitePool,
    root: PathBuf,
    maintenance: PathBuf,
    source: String,
    store: RepositoryMemoryStore,
    repository_id: Uuid,
    workspace_id: Uuid,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "EVK fixture"]);
        git(&root, &["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(root.join("source.rs"), "authoritative source").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "integrated source"]);
        let source = git(&root, &["rev-parse", "HEAD"]);
        let maintenance = temp.path().join("maintenance/repo");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "wiki",
                maintenance.to_str().unwrap(),
            ],
        );
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        let repository_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO repos (id, path, name, display_name) VALUES (?, ?, 'repo', 'Fixture')",
        )
        .bind(repository_id)
        .bind(root.to_str())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workspaces (id, branch, container_ref) VALUES (?, 'wiki', ?)")
            .bind(workspace_id)
            .bind(maintenance.parent().unwrap().to_str())
            .execute(&pool)
            .await
            .unwrap();
        Self {
            _temp: temp,
            pool,
            root,
            maintenance,
            source,
            store,
            repository_id,
            workspace_id,
        }
    }

    async fn reserve(&self) -> Uuid {
        let id = Uuid::new_v4();
        let mut graph = workflow::templates::openwiki_bootstrap().graph;
        for node in &mut graph.nodes {
            if node.id == "review" {
                node.data.prompt_template =
                    Some(openwiki::bootstrap::review_prompt(&self.maintenance, "ja"));
            }
        }
        reserve_repository_workflow(&self.pool, id, self.repository_id, self.workspace_id, graph)
            .await
            .unwrap();
        id
    }
}

struct FakeBootstrap<'a> {
    fixture: &'a Fixture,
    refine: bool,
    refute: bool,
    fail: Option<&'static str>,
    calls: Mutex<Vec<AgentNodeRequest>>,
    publications: AtomicUsize,
}

#[async_trait]
impl WorkflowAgentExecutor for FakeBootstrap<'_> {
    fn owns_repository_execution(&self) -> bool {
        true
    }

    async fn run_agent(&self, request: AgentNodeRequest) -> Result<AgentNodeExecution, ApiError> {
        let fixture = self.fixture;
        assert_eq!(
            git(&fixture.maintenance, &["rev-parse", "HEAD"]),
            fixture.source
        );
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), fixture.source);
        assert_eq!(self.publications.load(Ordering::SeqCst), 0);
        if self.fail == Some(request.node_id.as_str()) {
            return Err(ApiError::BadRequest("fixture phase failure".into()));
        }
        let output = match request.node_id.as_str() {
            "generate" => {
                std::fs::create_dir(fixture.maintenance.join("openwiki")).unwrap();
                std::fs::write(
                    fixture.maintenance.join("openwiki/INSTRUCTIONS.md"),
                    include_bytes!("../../../../assets/openwiki-instructions.md"),
                )
                .unwrap();
                std::fs::write(
                    fixture.maintenance.join("openwiki/index.md"),
                    "Generated Wiki",
                )
                .unwrap();
                "SENSITIVE_GENERATOR_CONVERSATION".into()
            }
            "review" => {
                assert!(!request.prompt.contains("SENSITIVE_GENERATOR_CONVERSATION"));
                assert!(!request.prompt.contains("Direct Upstream Handoff"));
                assert!(request.selected_skills.is_none());
                let raw = if self.refine {
                    json!({"version":1,"verdict":"needs_refinement","summary":"A material gap","findings":[{"severity":"material","title":"Missing lifecycle","description":"Document cancellation","evidencePaths":["source.rs"],"recommendedAction":"expand_page"}]})
                } else { json!({"version":1,"verdict":"pass","findings":[],"summary":"No material gaps"}) }.to_string();
                CoverageReview::parse(&raw).unwrap();
                raw
            }
            "refine" => {
                assert!(self.refine);
                let raw: String = sqlx::query_scalar("SELECT output_text FROM node_executions WHERE run_id = ? AND node_id = 'review' AND status = 'succeeded'")
                    .bind(request.run_id).fetch_one(&fixture.pool).await?;
                let review = CoverageReview::parse(&raw).unwrap();
                assert!(
                    openwiki::bootstrap::writer_prompt(&fixture.maintenance, "ja", Some(&review))
                        .contains("force=true")
                );
                if !self.refute {
                    std::fs::write(
                        fixture.maintenance.join("openwiki/index.md"),
                        "Refined Wiki",
                    )
                    .unwrap();
                }
                let report = json!({"version":1,"summary":"Checked the finding","resolutions":[{
                    "findingIndex":0,"disposition":if self.refute {"refuted"} else {"fixed"},
                    "reason":"Verified against source.rs","wikiPaths":["openwiki/index.md"],"evidencePaths":["source.rs"]
                }]}).to_string();
                let report =
                    openwiki::bootstrap::RefinementReport::parse(&report, &review).unwrap();
                report.validate_files(&fixture.maintenance).unwrap();
                serde_json::to_string(&report).unwrap()
            }
            _ => panic!("unexpected agent node"),
        };
        let session_id = request.session_id.unwrap();
        let agent_run_id = Uuid::new_v4();
        // The fake host supplies durable identities; real host protocol and
        // sandbox behaviour have separate adapter/Native Audit tests.
        sqlx::query("INSERT INTO agent_runs (id, session_id, workspace_id, request_id, idempotency_key, correlation_id, schema_version, payload_version, runtime_profile_id, provider_id, workspace_mode, workspace_path, request_envelope) VALUES (?, ?, ?, ?, ?, ?, 1, 1, 'codex:default', 'codex', 'isolated_worktree', ?, '{}')")
            .bind(agent_run_id).bind(session_id).bind(fixture.workspace_id).bind(Uuid::new_v4()).bind(agent_run_id.to_string()).bind(request.run_id).bind(fixture.maintenance.to_str()).execute(&fixture.pool).await?;
        let node_id = request.orchestration_node_execution_id;
        self.calls.lock().unwrap().push(request);
        Ok(AgentNodeExecution::Completed {
            session_id,
            orchestration_node_execution_id: node_id,
            agent_run_id,
            output_text: output,
        })
    }

    async fn publish_repository(&self, run_id: Uuid) -> Result<Option<String>, ApiError> {
        if self.fail == Some("publish") {
            return Err(ApiError::BadRequest("fixture publication failure".into()));
        }
        let f = self.fixture;
        let result = openwiki::publish_validated_wiki(
            &git::GitService::new(),
            &f.store,
            &openwiki::WikiPublicationRequest {
                repository_root: &f.root,
                maintenance_root: &f.maintenance,
                maintenance_branch: "wiki",
                target_branch: "main",
                source_commit: &f.source,
                run_id,
            },
        )
        .map_err(orchestration_api_error)?;
        assert!(
            !result.1,
            "New Wiki must not publish as a no-op: status={}, paths={:?}",
            git(&f.maintenance, &["status", "--short"]),
            openwiki::publication_paths(&git::GitService::new(), &f.maintenance, &f.source)
        );
        self.publications.fetch_add(1, Ordering::SeqCst);
        Ok(Some("Published".into()))
    }
}

#[tokio::test]
async fn bootstrap_cleanup_restores_and_releases_only_after_all_children_exit() {
    use utils::repository_memory::{
        OpenWikiBootstrapOwner, OpenWikiBootstrapPhase, RepositoryMemoryState, RepositoryWikiStatus,
    };
    let f = Fixture::new().await;
    let run_id = f.reserve().await;
    let repo = db::models::repo::Repo::find_by_id(&f.pool, f.repository_id)
        .await
        .unwrap()
        .unwrap();
    let service = OrchestrationService::new(f.pool.clone(), Arc::new(NoopAgentRunPort));
    let state = RepositoryMemoryState {
        enabled: true,
        status: RepositoryWikiStatus::Initializing,
        maintenance_workspace_id: Some(f.workspace_id),
        active_source_commit: Some(f.source.clone()),
        bootstrap: Some(OpenWikiBootstrapOwner {
            workflow_run_id: run_id,
            server_instance_id: Uuid::new_v4(),
            phase: OpenWikiBootstrapPhase::CleaningUp,
            child: None,
            review_fingerprint: None,
        }),
        error: Some("fixture failure".into()),
        ..Default::default()
    };
    f.store.save_state(&state).unwrap();
    openwiki::setup::prepare(&f.store, f.workspace_id, &f.maintenance, &f.source).unwrap();
    // An unlinked host is still sufficient to prevent instruction restoration.
    let session: Uuid =
        sqlx::query_scalar("SELECT id FROM sessions WHERE workspace_id = ? LIMIT 1")
            .bind(f.workspace_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO agent_runs (id, session_id, workspace_id, request_id, idempotency_key, correlation_id, schema_version, payload_version, runtime_profile_id, provider_id, workspace_mode, workspace_path, request_envelope, status) VALUES (?, ?, ?, ?, 'cleanup-host', ?, 1, 1, 'codex:default', 'codex', 'isolated_worktree', ?, '{}', 'running')")
        .bind(Uuid::new_v4()).bind(session).bind(f.workspace_id).bind(Uuid::new_v4()).bind(run_id).bind(f.maintenance.to_str()).execute(&f.pool).await.unwrap();
    assert!(
        super::super::bootstrap::cleanup_with_service(&f.pool, &repo, &f.store, &service)
            .await
            .is_err()
    );
    assert!(f.store.state().unwrap().bootstrap.is_some());
    assert_eq!(
        f.store.wiki_setup(f.workspace_id).unwrap().unwrap().phase,
        utils::repository_memory::WikiSetupPhase::Prepared
    );
    sqlx::query("UPDATE agent_runs SET status = 'failed'")
        .execute(&f.pool)
        .await
        .unwrap();
    super::super::bootstrap::cleanup_with_service(&f.pool, &repo, &f.store, &service)
        .await
        .unwrap();
    let result = f.store.state().unwrap();
    assert!(result.bootstrap.is_none() && result.active_run_id.is_none());
    assert_eq!(result.status, RepositoryWikiStatus::Error);
    assert_eq!(
        f.store.wiki_setup(f.workspace_id).unwrap().unwrap().phase,
        utils::repository_memory::WikiSetupPhase::Restored
    );
    assert_eq!(git(&f.maintenance, &["rev-parse", "HEAD"]), f.source);
    assert_eq!(git(&f.root, &["rev-parse", "main"]), f.source);
    assert_eq!(
        get_workflow_run_response(&f.pool, run_id)
            .await
            .unwrap()
            .status,
        WorkflowRunStatus::Failed
    );
    assert!(f.store.publication(run_id).unwrap().is_none());
}

#[tokio::test]
async fn bootstrap_validation_gate_survives_generic_recovery_and_does_not_block_cancel() {
    use executors::runtime::{
        AgentEventStream, AgentRunPort, AgentRunPortError, AgentRunPortSnapshot,
        OrchestrationRunStatus, RunState,
    };
    struct CompletedPort {
        pool: SqlitePool,
    }
    #[async_trait]
    impl AgentRunPort for CompletedPort {
        async fn create(
            &self,
            _: AgentRunRequestEnvelope,
            _: RunAttemptRequest,
        ) -> Result<Uuid, AgentRunPortError> {
            panic!("recovery must not create a paid child")
        }
        async fn control(&self, _: AgentRunPortCommandEnvelope) -> Result<(), AgentRunPortError> {
            Ok(())
        }
        async fn subscribe(&self, id: Uuid) -> Result<AgentEventStream, AgentRunPortError> {
            Err(AgentRunPortError::NotFound(id))
        }
        async fn query(&self, id: Uuid) -> Result<AgentRunPortSnapshot, AgentRunPortError> {
            let (session_id, workspace_id): (Uuid, Uuid) =
                sqlx::query_as("SELECT session_id, workspace_id FROM agent_runs WHERE id = ?")
                    .bind(id)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap();
            let mut state = RunState::pending(&AgentRunRequestEnvelope {
                schema_version: AGENT_REQUEST_SCHEMA_VERSION,
                payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
                request_id: id,
                idempotency_key: id.to_string(),
                session_id,
                agent_run_id: id,
                turn_id: id,
                correlation_id: id,
                intent: AgentRunIntent::Initial,
                runtime_profile_id: "codex:default".into(),
                provider_id: "codex".into(),
                workspace: WorkspaceReference {
                    workspace_id,
                    mode: WorkspaceMode::IsolatedWorktree,
                    path: ".".into(),
                },
                input: CanonicalMessage {
                    message_id: id,
                    role: AgentRuntimeMessageRole::User,
                    content: "fixture".into(),
                },
                created_at: Utc::now(),
            });
            state.status = AgentRunStatus::Succeeded;
            Ok(AgentRunPortSnapshot {
                agent_run_id: id,
                state,
            })
        }
    }
    for gated in [true, false] {
        let f = Fixture::new().await;
        let run_id = f.reserve().await;
        let executor = FakeBootstrap {
            fixture: &f,
            refine: false,
            refute: false,
            fail: None,
            calls: Mutex::new(Vec::new()),
            publications: AtomicUsize::new(0),
        };
        drive_repository_workflow(&f.pool, run_id, &executor)
            .await
            .unwrap();
        let run = get_workflow_run_response(&f.pool, run_id).await.unwrap();
        let orchestration_id = run.orchestration_run_id.unwrap();
        if !gated {
            let mut plan: sqlx::types::Json<executors::runtime::OrchestrationPlanSnapshot> =
                sqlx::query_scalar("SELECT plan_snapshot FROM orchestration_runs WHERE id = ?")
                    .bind(orchestration_id)
                    .fetch_one(&f.pool)
                    .await
                    .unwrap();
            for node in &mut plan.nodes {
                node.requires_product_validation = false;
            }
            sqlx::query("UPDATE orchestration_runs SET plan_snapshot = ? WHERE id = ?")
                .bind(plan)
                .bind(orchestration_id)
                .execute(&f.pool)
                .await
                .unwrap();
        }
        for node in run.nodes.iter().filter(|node| node.agent_run_id.is_some()) {
            db::models::orchestration::OrchestrationAgentRunLinkRecord::persist(
                &f.pool,
                Uuid::new_v4(),
                orchestration_id,
                node.orchestration_node_execution_id.unwrap(),
                node.agent_run_id.unwrap(),
                &node.node_id,
            )
            .await
            .unwrap();
        }
        let service = OrchestrationService::new(
            f.pool.clone(),
            Arc::new(CompletedPort {
                pool: f.pool.clone(),
            }),
        );
        service
            .reconcile_run(orchestration_id, orchestration_id)
            .await
            .unwrap();
        let successful: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orchestration_node_executions WHERE orchestration_run_id = ? AND status = 'succeeded'").bind(orchestration_id).fetch_one(&f.pool).await.unwrap();
        assert_eq!(successful, if gated { 0 } else { 2 });
        service
            .cancel(orchestration_id, orchestration_id)
            .await
            .unwrap();
        service
            .reconcile_run(orchestration_id, orchestration_id)
            .await
            .unwrap();
        let status: OrchestrationRunStatus =
            sqlx::query_scalar("SELECT status FROM orchestration_runs WHERE id = ?")
                .bind(orchestration_id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(status, OrchestrationRunStatus::Cancelled);
    }
}

#[tokio::test]
async fn bootstrap_scope_migration_preserves_existing_issue_runs_and_references() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut before = sqlx::migrate!("../db/migrations");
    before.migrations = std::borrow::Cow::Owned(
        before
            .iter()
            .filter(|migration| migration.version < 20260913120000)
            .cloned()
            .collect(),
    );
    before.run(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO projects (id, name) VALUES (X'00000000000000000000000000000001', 'Old project');
        INSERT INTO local_project_statuses (id, project_id, name, color, sort_order) VALUES (X'00000000000000000000000000000002', X'00000000000000000000000000000001', 'Todo', 'blue', 1);
        INSERT INTO local_issues (id, project_id, issue_number, simple_id, status_id, title, sort_order) VALUES (X'00000000000000000000000000000003', X'00000000000000000000000000000001', 1, 'OLD-1', X'00000000000000000000000000000002', 'Existing Issue', 1);
        INSERT INTO workflows (id, source, name, graph_json) VALUES (X'00000000000000000000000000000004', 'system', 'Old template', '{}');
        INSERT INTO workflow_attempts (id, project_id, issue_id, workflow_id, name) VALUES (X'00000000000000000000000000000005', X'00000000000000000000000000000001', X'00000000000000000000000000000003', X'00000000000000000000000000000004', 'Old attempt');
        INSERT INTO workflow_runs (id, workflow_id, issue_id, input_text, graph_snapshot, attempt_id) VALUES (X'00000000000000000000000000000006', X'00000000000000000000000000000004', X'00000000000000000000000000000003', 'old input', '{\"snapshot\":true}', X'00000000000000000000000000000005');
        UPDATE workflow_attempts SET latest_run_id = X'00000000000000000000000000000006';
        INSERT INTO node_executions (id, run_id, node_id, node_type, output_text) VALUES (X'00000000000000000000000000000007', X'00000000000000000000000000000006', 'old-agent', 'agent', 'preserved artifact');")
        .execute(&pool).await.unwrap();
    sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
    let preserved: (Uuid, Option<Uuid>, String, String) = sqlx::query_as(
        "SELECT issue_id, repository_id, input_text, graph_snapshot FROM workflow_runs",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        preserved,
        (
            Uuid::from_u128(3),
            None,
            "old input".into(),
            "{\"snapshot\":true}".into()
        )
    );
    let artifact: String = sqlx::query_scalar("SELECT output_text FROM node_executions ne JOIN workflow_attempts wa ON wa.latest_run_id = ne.run_id").fetch_one(&pool).await.unwrap();
    assert_eq!(artifact, "preserved artifact");
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        sqlx::query("UPDATE workflow_runs SET issue_id = NULL")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn bootstrap_pass_and_refine_use_existing_runner_and_publish_once() {
    for (refine, refute) in [(false, false), (true, false), (true, true)] {
        let f = Fixture::new().await;
        let run_id = f.reserve().await;
        let executor = FakeBootstrap {
            fixture: &f,
            refine,
            refute,
            fail: None,
            calls: Mutex::new(Vec::new()),
            publications: AtomicUsize::new(0),
        };
        drive_repository_workflow(&f.pool, run_id, &executor)
            .await
            .unwrap();
        let run = get_workflow_run_response(&f.pool, run_id).await.unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Succeeded);
        assert_eq!(run.issue_id, None);
        assert_eq!(run.attempt_id, None);
        assert_eq!(run.repository_id, Some(f.repository_id));
        let calls = executor.calls.lock().unwrap().clone();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.node_id.as_str())
                .collect::<Vec<_>>(),
            if refine {
                vec!["generate", "review", "refine"]
            } else {
                vec!["generate", "review"]
            }
        );
        assert!(calls.iter().all(|call| call.workspace_id == f.workspace_id));
        assert_eq!(
            calls
                .iter()
                .map(|call| call.session_id)
                .collect::<HashSet<_>>()
                .len(),
            calls.len()
        );
        assert_eq!(
            run.nodes
                .iter()
                .filter_map(|node| node.agent_run_id)
                .collect::<HashSet<_>>()
                .len(),
            calls.len()
        );
        assert_eq!(executor.publications.load(Ordering::SeqCst), 1);
        if refute {
            assert_eq!(
                git(&f.root, &["show", "main:openwiki/index.md"]),
                "Generated Wiki"
            );
        }
        assert_eq!(
            git(
                &f.root,
                &["rev-list", "--count", &format!("{}..main", f.source)]
            ),
            "1"
        );
        assert_eq!(
            git(&f.root, &["diff", "--name-only", &f.source, "main"]),
            "openwiki/INSTRUCTIONS.md\nopenwiki/index.md"
        );
        assert_eq!(
            run.nodes.len(),
            7,
            "skip semantics must not manufacture another Publish iteration"
        );
        let snapshot: String = sqlx::query_scalar("SELECT graph_json FROM workflows WHERE id = ?")
            .bind(run.workflow_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert!(
            !snapshot.contains("session_id"),
            "global system template must stay session-free"
        );
    }
}

#[tokio::test]
async fn bootstrap_phase_failures_never_reach_publication_or_reuse_retry() {
    for phase in ["generate", "review", "refine", "publish"] {
        let f = Fixture::new().await;
        let run_id = f.reserve().await;
        let executor = FakeBootstrap {
            fixture: &f,
            refine: true,
            refute: false,
            fail: Some(phase),
            calls: Mutex::new(Vec::new()),
            publications: AtomicUsize::new(0),
        };
        let _ = drive_repository_workflow(&f.pool, run_id, &executor).await;
        assert_eq!(executor.publications.load(Ordering::SeqCst), 0);
        assert_eq!(git(&f.root, &["rev-parse", "main"]), f.source);
        assert_eq!(git(&f.maintenance, &["rev-parse", "HEAD"]), f.source);
        assert!(
            retry_workflow_node(&f.pool, run_id, phase, &executor)
                .await
                .is_err()
        );
    }
}
