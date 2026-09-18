use db::models::{
    execution_process::CreateExecutionProcess,
    execution_process_repo_state::CreateExecutionProcessRepoState,
    integration::{IntegrationPayload, IntegrationSelection, IntegrationSource},
    workspace::CreateWorkspace,
};
use serde_json::json;

use super::*;

async fn fixture() -> (SqlitePool, IntegrationRun, ExecutionProcess, ExecutorAction) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
    let repo = Repo::find_or_create(&pool, Path::new("/test-validation"), "source")
        .await
        .unwrap();
    let ws = Workspace::create(
        &pool,
        &CreateWorkspace {
            branch: "test".into(),
            name: None,
        },
        Uuid::new_v4(),
    )
    .await
    .unwrap();
    let session = Session::create(
        &pool,
        &CreateSession {
            executor: None,
            name: None,
        },
        Uuid::new_v4(),
        ws.id,
    )
    .await
    .unwrap();
    let action = ExecutorAction::new(
        ExecutorActionType::ScriptRequest(ScriptRequest {
            script: "node --test".into(),
            language: ScriptRequestLanguage::Bash,
            context: ScriptContext::IntegrationValidation,
            working_dir: Some(format!("{}/.", repo.name)),
        }),
        None,
    );
    let oid = "a".repeat(40);
    let process = ExecutionProcess::create(
        &pool,
        &CreateExecutionProcess {
            session_id: session.id,
            executor_action: action.clone(),
            run_reason: ExecutionProcessRunReason::IntegrationValidation,
        },
        Uuid::new_v4(),
        &[CreateExecutionProcessRepoState {
            repo_id: repo.id,
            before_head_commit: Some(oid.clone()),
            after_head_commit: None,
            merge_commit: None,
        }],
    )
    .await
    .unwrap();
    ExecutionProcess::update_completion(
        &pool,
        process.id,
        ExecutionProcessStatus::Completed,
        Some(0),
    )
    .await
    .unwrap();
    let run = IntegrationRun {
        id: Uuid::new_v4(),
        request_key: "test".into(),
        project_id: Uuid::new_v4(),
        repository_id: repo.id,
        storage_identity: "/git/common".into(),
        target_ref: "refs/heads/main".into(),
        status: "validating".into(),
        workspace_id: Some(ws.id),
        session_id: Some(session.id),
        agent_run_id: None,
        payload: sqlx::types::Json(IntegrationPayload {
            sources: vec![IntegrationSource {
                selection: IntegrationSelection {
                    card_id: Uuid::new_v4(),
                    workspace_id: ws.id,
                    expected_commit: oid.clone(),
                },
                branch: "source".into(),
                commit: oid.clone(),
                title: "request".into(),
                description: None,
                status_id: Uuid::new_v4(),
                requirements_revision: 1,
                event_ids: vec![],
                related_workspaces: vec![ws.id],
                done_result: None,
            }],
            result_commit: Some(oid),
            validation: vec![IntegrationValidation {
                command: "node --test".into(),
                cwd: ".".into(),
                required: true,
                evidence: "package.json test + selected acceptance".into(),
                environment_requirements: "isolated in-memory fixtures; no network".into(),
                execution_process_id: Some(process.id),
                exit_code: Some(0),
                result: Some("passed".into()),
            }],
            ..Default::default()
        }),
        cancel_requested: false,
        error: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    (pool, run, process, action)
}

#[tokio::test]
async fn host_proof_requires_after_head_and_matches_plan_not_self_report() {
    let (pool, mut run, process, action) = fixture().await;
    // A zero exit and claimed success are insufficient before finalisation.
    assert!(verify_validation(&pool, &run).await.is_err());
    ExecutionProcessRepoState::update_after_head_commit(
        &pool,
        process.id,
        run.repository_id,
        run.payload.result_commit.as_deref().unwrap(),
    )
    .await
    .unwrap();
    verify_validation(&pool, &run).await.unwrap();
    for replacement in ["b".repeat(40), String::new()] {
        ExecutionProcessRepoState::update_after_head_commit(
            &pool,
            process.id,
            run.repository_id,
            &replacement,
        )
        .await
        .unwrap();
        assert!(verify_validation(&pool, &run).await.is_err());
    }
    ExecutionProcessRepoState::update_after_head_commit(
        &pool,
        process.id,
        run.repository_id,
        run.payload.result_commit.as_deref().unwrap(),
    )
    .await
    .unwrap();
    let mut changed = action.clone();
    let ExecutorActionType::ScriptRequest(script) = &mut changed.typ;
    script.working_dir = Some("wrong-repo".into());
    sqlx::query("UPDATE execution_processes SET executor_action=? WHERE id=?")
        .bind(sqlx::types::Json(&changed))
        .bind(process.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(verify_validation(&pool, &run).await.is_err());
    changed = action.clone();
    changed.next_action = Some(Box::new(action.clone()));
    sqlx::query("UPDATE execution_processes SET executor_action=? WHERE id=?")
        .bind(sqlx::types::Json(&changed))
        .bind(process.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(verify_validation(&pool, &run).await.is_err());
    sqlx::query("UPDATE execution_processes SET executor_action=? WHERE id=?")
        .bind(sqlx::types::Json(&action))
        .bind(process.id)
        .execute(&pool)
        .await
        .unwrap();
    run.payload.validation[0].command = "node --test subset.test.js".into();
    assert!(verify_validation(&pool, &run).await.is_err());
    run.payload.validation[0].command = "node --test".into();
    ExecutionProcess::update_completion(
        &pool,
        process.id,
        ExecutionProcessStatus::Completed,
        Some(1),
    )
    .await
    .unwrap();
    assert!(verify_validation(&pool, &run).await.is_err());
    // Replacing mandatory with optional cannot make a plan with no required
    // verification pass; optional checks still need real evidence when present.
    run.payload.validation[0].required = false;
    assert!(verify_validation(&pool, &run).await.is_err());
}

#[tokio::test]
async fn proposal_rejects_changed_selection_empty_plan_and_placeholder_success() {
    let (_pool, run, _, _) = fixture().await;
    let mut value = json!({"outcome":"ready","selected_card_ids":[run.payload.sources[0].selection.card_id],
        "summary":"all selected requirements integrated; host tests pending", "validation":[{"command":"node --test","cwd":".","required":true,"evidence":"package.json","environment_requirements":"no external services"}],"exclusions":[],"semantics":{"goal":"combined request","summary":"compatibility resolution"}});
    proposal(&value.to_string(), &run).unwrap();
    value["selected_card_ids"] = json!([]);
    assert!(proposal(&value.to_string(), &run).is_err());
    value["selected_card_ids"] = json!([run.payload.sources[0].selection.card_id]);
    for command in ["true", "exit 0", ":", " "] {
        value["validation"][0]["command"] = json!(command);
        assert!(proposal(&value.to_string(), &run).is_err());
    }
    value["validation"][0]["command"] = json!("node --test");
    value["validation"][0]["cwd"] = json!("../peer");
    assert!(proposal(&value.to_string(), &run).is_err());
    value["validation"] = json!([]);
    assert!(proposal(&value.to_string(), &run).is_err());
}

#[tokio::test]
async fn dispatched_prompt_uses_resolved_api_port_and_selected_identity() {
    let (pool, run, _, _) = fixture().await;
    let repo = Repo::find_by_id(&pool, run.repository_id)
        .await
        .unwrap()
        .unwrap();
    let body = prompt(&run, &repo, Path::new("/test"), Some(4021)).unwrap();
    assert!(body.contains(&format!(
        "http://127.0.0.1:4021/api/repos/{}/parallel-context",
        repo.id
    )));
    assert!(body.contains(&run.payload.sources[0].selection.card_id.to_string()));
    assert!(body.contains(&run.payload.sources[0].commit));
    let unavailable = prompt(&run, &repo, Path::new("/test"), None).unwrap();
    assert!(!unavailable.contains("http://127.0.0.1:"));
    assert!(unavailable.contains("do not invent an endpoint"));
}

#[tokio::test]
async fn disabled_memory_does_not_publish_events_or_integrations() {
    let (pool, mut run, _, _) = fixture().await;
    let temp = tempfile::tempdir().unwrap();
    let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
    store
        .save_state(&utils::repository_memory::RepositoryMemoryState::default())
        .unwrap();
    let repo = Repo::find_by_id(&pool, run.repository_id)
        .await
        .unwrap()
        .unwrap();
    run.payload.sources.clear();
    run.payload.base_commit = Some("b".repeat(40));
    run.payload.integration_manifest = Some(ChangeManifest {
        version: 1,
        event_id: run.id,
        repository_id: repo.id,
        workspace_id: run.workspace_id.unwrap(),
        task_id: None,
        created_at: chrono::Utc::now(),
        base_commit: run.payload.base_commit.clone().unwrap(),
        source_commit: run.payload.result_commit.clone().unwrap(),
        target_branch: Some("main".into()),
        changed_paths: vec!["source.txt".into()],
        semantics: SemanticChanges {
            goal: "combine selected sources".into(),
            summary: "integrate".into(),
            ..Default::default()
        },
    });
    assert_eq!(
        record_memory_integration_at(&git::GitService::new(), &repo, &run, &store).unwrap(),
        "disabled"
    );
    assert!(
        store.events().unwrap().is_empty(),
        "disabled Memory must not acquire new events"
    );
    assert!(
        store.integrations().unwrap().is_empty(),
        "Git/Done receipts belong to Integration, not disabled Memory"
    );
    // Old absent/corrupt semantic inputs must not hold Git/Card post-processing
    // hostage when the repository explicitly disabled memory.
    run.payload.sources = fixture().await.1.payload.sources.clone();
    run.payload.sources[0].event_ids = vec![Uuid::new_v4()];
    assert_eq!(
        record_memory_integration_at(&git::GitService::new(), &repo, &run, &store).unwrap(),
        "disabled"
    );
}

#[tokio::test]
async fn semantic_outbox_uses_only_frozen_events_and_rejects_changed_or_missing_inputs() {
    let (pool, mut run, _, _) = fixture().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    let persistent = temp.path().join("persistent");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&persistent).unwrap();
    let cmd = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    cmd(&["init", "-b", "main"]);
    let commit = |name: &str| {
        cmd(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            name,
        ]);
        cmd(&["rev-parse", "HEAD"])
    };
    let base = commit("base");
    let selected = commit("selected source");
    let later = commit("later source not selected");
    let mut repo = Repo::find_by_id(&pool, run.repository_id)
        .await
        .unwrap()
        .unwrap();
    repo.path = root;
    let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
    let state = utils::repository_memory::RepositoryMemoryState {
        enabled: true,
        target_branch: Some("main".into()),
        ..Default::default()
    };
    store.save_state(&state).unwrap();
    let event = ChangeManifest {
        version: 1,
        event_id: Uuid::new_v4(),
        repository_id: repo.id,
        workspace_id: run.payload.sources[0].selection.workspace_id,
        task_id: None,
        created_at: chrono::Utc::now(),
        base_commit: base.clone(),
        source_commit: selected.clone(),
        target_branch: Some("main".into()),
        changed_paths: vec!["source.txt".into()],
        semantics: SemanticChanges {
            goal: "selected source intent".into(),
            summary: "selected change".into(),
            ..Default::default()
        },
    };
    store.publish_event(&event).unwrap();
    let mut newer = event.clone();
    newer.event_id = Uuid::new_v4();
    newer.source_commit = later.clone();
    store.publish_event(&newer).unwrap();
    let mut peer = newer.clone();
    peer.event_id = Uuid::new_v4();
    peer.workspace_id = Uuid::new_v4();
    peer.target_branch = Some("another-target".into());
    store.publish_event(&peer).unwrap();
    let source = &mut run.payload.sources[0];
    source.commit = selected.clone();
    source.selection.expected_commit = selected.clone();
    source.event_ids = vec![event.event_id, event.event_id];
    run.payload.base_commit = Some(base);
    run.payload.result_commit = Some(selected);
    let git = git::GitService::new();
    for _ in 0..2 {
        assert_eq!(
            record_memory_integration_at(&git, &repo, &run, &store).unwrap(),
            "pending existing OpenWiki reconciliation"
        );
        let records = store.integrations().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].event_ids, vec![event.event_id]);
        assert_eq!(records[0].target_branch, "main");
    }
    // Later source events cannot silently enter the frozen set, and a peer
    // event cannot be substituted even when its Git object exists here.
    for invalid in [newer.event_id, peer.event_id, Uuid::new_v4()] {
        let mut changed = run.clone();
        changed.payload.sources[0].event_ids = vec![invalid];
        assert!(record_memory_integration_at(&git, &repo, &changed, &store).is_err());
    }
    let mut changed_target = run.clone();
    changed_target.target_ref = "refs/heads/another-target".into();
    assert!(record_memory_integration_at(&git, &repo, &changed_target, &store).is_err());
    // Corrupt a required frozen record: discovery may partially display other
    // records, but strict Integration publication may not ignore this failure.
    std::fs::write(
        persistent
            .join("knowledge/events")
            .join(format!("{}.json", event.event_id)),
        "{invalid",
    )
    .unwrap();
    assert!(record_memory_integration_at(&git, &repo, &run, &store).is_err());
    assert_eq!(
        store.integrations().unwrap()[0].event_ids,
        vec![event.event_id]
    );
}

#[tokio::test]
async fn git_publication_survives_receipt_failure_without_a_second_merge() {
    let (pool, mut run, _, _) = fixture().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let cmd = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    cmd(&["init", "-b", "main"]);
    std::fs::write(root.join("source.txt"), "base").unwrap();
    cmd(&["add", "source.txt"]);
    cmd(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "base",
    ]);
    let base = cmd(&["rev-parse", "HEAD"]);
    cmd(&["checkout", "-b", "result"]);
    std::fs::write(root.join("source.txt"), "validated result").unwrap();
    cmd(&["add", "source.txt"]);
    cmd(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "result",
    ]);
    let result = cmd(&["rev-parse", "HEAD"]);
    cmd(&["checkout", "main"]);
    sqlx::query("UPDATE repos SET path=? WHERE id=?")
        .bind(root.to_str().unwrap())
        .bind(run.repository_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects(id,name) VALUES(?,'publication')")
        .bind(run.project_id)
        .execute(&pool)
        .await
        .unwrap();
    run.payload.base_commit = Some(base.clone());
    run.payload.result_commit = Some(result.clone());
    run.payload.publication_intent = true;
    run.status = "publishing".into();
    sqlx::query("INSERT INTO integration_runs(id,request_key,project_id,repository_id,storage_identity,target_ref,status,workspace_id,session_id,payload) VALUES(?,?,?,?,?,?,'publishing',?,?,?)")
        .bind(run.id).bind(&run.request_key).bind(run.project_id).bind(run.repository_id).bind(root.to_str().unwrap()).bind(&run.target_ref)
        .bind(run.workspace_id).bind(run.session_id).bind(&run.payload).execute(&pool).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_publication_receipt BEFORE UPDATE ON integration_runs WHEN json_extract(NEW.payload,'$.published')=1 BEGIN SELECT RAISE(ABORT,'simulated crash before receipt'); END;")
        .execute(&pool).await.unwrap();
    let repo = Repo::find_by_id(&pool, run.repository_id)
        .await
        .unwrap()
        .unwrap();
    let git = git::GitService::new();
    let targets = vec![root.to_path_buf()];
    assert!(
        publish_candidate(&git, &pool, &repo, &mut run, &targets)
            .await
            .is_err()
    );
    assert_eq!(cmd(&["rev-parse", "main"]), result);
    assert_eq!(
        std::fs::read_to_string(root.join("source.txt")).unwrap(),
        "validated result"
    );
    let mut recovered = IntegrationRun::find(&pool, run.id).await.unwrap();
    assert!(recovered.payload.publication_intent);
    assert!(!recovered.payload.published);
    assert!(
        recovered
            .payload
            .sources
            .iter()
            .all(|source| source.done_result.is_none())
    );
    assert_eq!(
        git.inspect_exact_publication(root, &run.target_ref, &base, &result, false, &targets)
            .unwrap(),
        git::publication::ExactPublicationState::Applied
    );
    sqlx::query("DROP TRIGGER fail_publication_receipt")
        .execute(&pool)
        .await
        .unwrap();
    recovered.payload.published = true;
    recovered.status = "post_processing".into();
    recovered.save(&pool).await.unwrap();
    assert_eq!(cmd(&["rev-list", "--count", "main"]), "2");
    // Recovery does not invoke the writer a second time, even for a repeated UI request.
    assert!(
        publish_candidate(&git, &pool, &repo, &mut recovered, &targets)
            .await
            .is_err()
    );
    assert_eq!(cmd(&["rev-parse", "result"]), result);
}
