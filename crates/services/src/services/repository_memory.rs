//! Repository-memory lifecycle attached to EVK's existing Git and shared-folder
//! services. No model client: semantic drafts come from the active coding host.
use std::{collections::HashSet, path::Path};

use anyhow::{Context, bail};
use db::models::{repo::Repo, workspace::Workspace};
use git::GitService;
use utils::repository_memory::{
    ChangeManifest, MemoryIntegration, ReconciliationResult, RepositoryMemoryRun,
    RepositoryMemoryStore, SourcePublication,
};
use uuid::Uuid;

pub fn begin_coding_run(
    repo: &Repo,
    workspace: &Workspace,
    run_id: Uuid,
    root: &Path,
    target_branch: &str,
    finalize_source: bool,
) -> anyhow::Result<Option<RepositoryMemoryRun>> {
    let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)? else {
        return Ok(None);
    };
    let state = store.state()?;
    if !state.enabled || state.maintenance_workspace_id == Some(workspace.id) {
        return Ok(None);
    }
    if let Some(existing) = store.run(run_id)? {
        return Ok(Some(existing));
    }
    let base_commit = GitService::new().get_head_info(root)?.oid;
    // Freshness is discovery, not a publication gate. A malformed unrelated
    // peer event must not prevent normal source work; strict readers below still
    // reject it when an integration/reconciliation consumes durable evidence.
    let pending = (|| -> anyhow::Result<bool> {
        Ok(unresolved_integration(&store)? || !pending_events(&store, target_branch)?.is_empty())
    })();
    let wiki_status = if let Ok(pending) = pending {
        state.derived_status(
            root.join("openwiki/index.md").is_file(),
            pending,
            state
                .wiki_commit
                .as_deref()
                .or(state.source_commit.as_deref())
                == Some(&base_commit),
        )
    } else {
        tracing::warn!(repository_id=%repo.id,"Repository Wiki freshness unavailable; strict publication evidence remains required");
        utils::repository_memory::RepositoryWikiStatus::Error
    };
    let run = RepositoryMemoryRun {
        run_id,
        repository_id: repo.id,
        workspace_id: workspace.id,
        task_id: workspace.task_id,
        created_at: chrono::Utc::now(),
        base_commit,
        target_branch: target_branch.into(),
        repository_path: root.into(),
        memory_path: store.memory_path(workspace.id),
        draft_path: store.draft_path(run_id),
        wiki_status,
        finalize_source,
    };
    store.save_run(&run)?;
    Ok(Some(run))
}

pub fn canonical_wiki_path(path: &str) -> bool {
    path == "openwiki" || path.starts_with("openwiki/")
}

/// Detect committed AND uncommitted changes, including renamed/deleted Wiki
/// files. Never discard them; reject completion/integration with a clear error.
pub fn guard_normal_workspace(
    git: &GitService,
    root: &Path,
    base: &str,
) -> anyhow::Result<Vec<String>> {
    let commit = base.parse().context("invalid frozen Git base")?;
    let mut paths: Vec<_> = git
        .get_diff_file_paths(root, &commit)?
        .into_iter()
        .collect();
    paths.sort();
    // A rename out of openwiki/ must not bypass a new-path-only file list.
    if paths.iter().any(|path| canonical_wiki_path(path))
        || !git
            .get_diffs(root, &commit, Some(&["openwiki"]))?
            .is_empty()
    {
        bail!(
            "Normal workspace modified canonical openwiki/. Source commit/integration is blocked; review and move Wiki edits to repository maintenance. No files were discarded."
        );
    }
    Ok(paths)
}

/// Called before consuming a queued follow-up. Immutable event identity makes
/// terminal notification replay harmless; a failed write can be retried.
pub fn complete_coding_run(
    store: &RepositoryMemoryStore,
    run: &RepositoryMemoryRun,
) -> anyhow::Result<Option<ChangeManifest>> {
    let _lock = store.try_workspace_completion_lock(run.workspace_id)?;
    let result = complete_coding_run_inner(store, run);
    store.record_coding_result(run, result.as_ref().err().map(|error| format!("{error:#}")))?;
    result
}

/// Terminal notifications are asynchronous. Reconcile a previous successful
/// turn before a new coding host can modify the same worktree. This also covers
/// a dropped notification or server restart without reading a later task's
/// changes into the older run's manifest. Distinct workspace IDs never wait on
/// each other's completion lock.
pub async fn complete_previous_coding_runs(
    pool: &sqlx::SqlitePool,
    store: &RepositoryMemoryStore,
    workspace_id: Uuid,
    next_run_id: Uuid,
) -> anyhow::Result<()> {
    let mut previous: Vec<_> = store
        .runs()?
        .into_iter()
        .filter(|run| run.workspace_id == workspace_id && run.run_id != next_run_id)
        .collect();
    previous.sort_by_key(|run| (run.created_at, run.run_id));
    for run in previous {
        let status: Option<executors::runtime::AgentRunStatus> =
            sqlx::query_scalar("SELECT status FROM agent_runs WHERE id = ?")
                .bind(run.run_id)
                .fetch_optional(pool)
                .await?;
        if status != Some(executors::runtime::AgentRunStatus::Succeeded) {
            continue;
        }
        // A terminal subscriber may already be completing this run. A brief,
        // bounded wait avoids making normal immediate follow-ups fail on it.
        for attempt in 0..3 {
            let (store, context) = (store.clone(), run.clone());
            let result =
                tokio::task::spawn_blocking(move || complete_coding_run(&store, &context)).await?;
            match result {
                Ok(_) => break,
                Err(error) if attempt < 2 && error.downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                Err(error) => return Err(error).with_context(|| format!(
                    "Previous successful run {} has unfinished source/Manifest publication. Resolve its semantic draft at {} before starting another coding task; source files were preserved",
                    run.run_id, run.draft_path.display()
                )),
            }
        }
    }
    Ok(())
}

fn complete_coding_run_inner(
    store: &RepositoryMemoryStore,
    run: &RepositoryMemoryRun,
) -> anyhow::Result<Option<ChangeManifest>> {
    if let Some(existing) = store.event(run.run_id)? {
        observe_already_integrated_source(store, run, &existing)?;
        return Ok(Some(existing));
    }
    if store.completed_without_change(run.run_id)? {
        return Ok(None);
    }
    if !run.finalize_source {
        store.mark_no_source_change(run.run_id)?;
        return Ok(None);
    }
    if let Some(publication) = store.source_publication(run.run_id)? {
        return finish_source_publication(store, run, publication).map(Some);
    }
    let git = GitService::new();
    let head = git.get_head_info(&run.repository_path)?;
    let branch_base = git.get_fork_point(&run.repository_path, &run.target_branch, &head.oid)?;
    // A direct-folder workspace can be on the integrated branch itself; its
    // current merge base would hide committed Wiki edits made by this run.
    let checked_out_branch = git.get_head_info(&run.repository_path)?.branch;
    let guard_base = if checked_out_branch == run.target_branch {
        &run.base_commit
    } else {
        &branch_base
    };
    guard_normal_workspace(&git, &run.repository_path, guard_base)?;
    let effective_base = if checked_out_branch != run.target_branch
        && git.is_ancestor(&run.repository_path, &run.base_commit, &branch_base)?
    {
        // Rebase/merge may have incorporated upstream source and Wiki since
        // launch. Attribute only this workspace's remaining branch changes.
        branch_base
    } else if git.is_ancestor(&run.repository_path, &run.base_commit, &head.oid)? {
        run.base_commit.clone()
    } else {
        branch_base
    };
    let mut paths: Vec<_> = git
        .get_diff_file_paths(&run.repository_path, &effective_base.parse()?)?
        .into_iter()
        .filter(|path| !canonical_wiki_path(path))
        .collect();
    paths.sort();
    if paths.is_empty() {
        store.mark_no_source_change(run.run_id)?;
        return Ok(None);
    }
    let semantics = store.read_draft(run.run_id)?.context("Source changed but the coding host did not supply a valid Change Manifest draft. Finish the semantic summary before integration.")?;
    let event = ChangeManifest {
        version: 1,
        event_id: run.run_id,
        repository_id: run.repository_id,
        workspace_id: run.workspace_id,
        task_id: run.task_id,
        created_at: chrono::Utc::now(),
        base_commit: effective_base,
        source_commit: head.oid,
        target_branch: Some(run.target_branch.clone()),
        changed_paths: paths,
        semantics,
    };
    let publication = SourcePublication {
        manifest: event,
        staged_tree: git.stage_source_snapshot(&run.repository_path)?,
    };
    // This record precedes Git mutation; after a crash, no mutable draft or
    // later worktree diff participates in identifying the old task's result.
    store.save_source_publication(&publication)?;
    finish_source_publication(store, run, publication).map(Some)
}

fn git_text(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!("Cannot verify source publication Git identity");
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn finish_source_publication(
    store: &RepositoryMemoryStore,
    run: &RepositoryMemoryRun,
    publication: SourcePublication,
) -> anyhow::Result<ChangeManifest> {
    let mut event = publication.manifest;
    if event.repository_id != run.repository_id || event.workspace_id != run.workspace_id {
        bail!("Source publication does not belong to this workspace/repository");
    }
    let root = &run.repository_path;
    let before = event.source_commit.clone();
    let marker = format!("EVK-Memory-Source: {}", run.run_id);
    let original_tree = git_text(root, &["rev-parse", &format!("{before}^{{tree}}")])?;
    if original_tree != publication.staged_tree {
        let range = format!("{before}..HEAD");
        let matches = git_text(
            root,
            &[
                "log",
                "--format=%H",
                "--fixed-strings",
                "--grep",
                &marker,
                &range,
                "--",
            ],
        )?;
        let matches: Vec<_> = matches.lines().collect();
        let commit = match matches.as_slice() {
            [commit] => (*commit).to_string(),
            [] => {
                let message = format!("{}\n\n{marker}", event.semantics.commit_message());
                GitService::new().commit_staged_snapshot(
                    root,
                    &message,
                    &before,
                    &publication.staged_tree,
                )?;
                git_text(root, &["rev-parse", "HEAD"])?
            }
            _ => {
                bail!("Ambiguous source publication identity; inspect Git history before retrying")
            }
        };
        let parents = git_text(root, &["show", "-s", "--format=%P", &commit])?;
        let tree = git_text(root, &["rev-parse", &format!("{commit}^{{tree}}")])?;
        if parents != before || tree != publication.staged_tree {
            bail!(
                "Source publication parent/tree differs from its frozen checkpoint; no event was acknowledged"
            );
        }
        event.source_commit = commit;
    }
    store.publish_event(&event)?;
    observe_already_integrated_source(store, run, &event)?;
    Ok(event)
}

fn observe_already_integrated_source(
    store: &RepositoryMemoryStore,
    run: &RepositoryMemoryRun,
    event: &ChangeManifest,
) -> anyhow::Result<()> {
    if store.integrations()?.iter().any(|record| {
        record.integrated_commit.is_some() && record.event_ids.contains(&event.event_id)
    }) {
        return Ok(());
    }
    let git = GitService::new();
    let target = git.get_branch_oid(&run.repository_path, &run.target_branch)?;
    if git.is_ancestor(&run.repository_path, &event.source_commit, &target)? {
        // Direct-folder work on the target, or a verified fast-forward, already
        // integrated this exact source commit. Squashes still need explicit IDs.
        store.save_integration(&MemoryIntegration {
            id: event.event_id,
            event_ids: vec![event.event_id],
            target_branch: run.target_branch.clone(),
            before_commit: event.base_commit.clone(),
            integrated_commit: Some(event.source_commit.clone()),
            source_error: None,
        })?;
    }
    Ok(())
}

pub fn prepare_integration(
    store: &RepositoryMemoryStore,
    workspace_id: Uuid,
    target_branch: &str,
    before_commit: String,
) -> anyhow::Result<MemoryIntegration> {
    let events = store.events()?;
    let integrated: HashSet<_> = store
        .integrations()?
        .into_iter()
        .filter(|record| record.integrated_commit.is_some())
        .flat_map(|record| record.event_ids)
        .collect();
    let integration = MemoryIntegration {
        id: Uuid::new_v4(),
        target_branch: target_branch.into(),
        before_commit,
        integrated_commit: None,
        source_error: None,
        event_ids: events
            .into_iter()
            .filter(|event| {
                event.workspace_id == workspace_id && !integrated.contains(&event.event_id)
            })
            .map(|event| event.event_id)
            .collect(),
    };
    store.save_integration(&integration)?;
    Ok(integration)
}

pub fn integration_commit_message(summary: &str, integration_id: Uuid) -> String {
    format!("{summary}\n\nEVK-Memory-Integration: {integration_id}")
}

/// External PR observations have no EVK-owned merge transaction carrying an
/// explicit event set. Use ancestry only as a conservative fallback: timestamps
/// or shared workspace membership do not prove that a later/unpushed change was
/// part of the PR. Unknown external squash/rebase membership remains unconsumed.
pub fn observe_external_integration(
    store: &RepositoryMemoryStore,
    root: &Path,
    integration_id: Uuid,
    workspace_id: Uuid,
    target_branch: &str,
    merge_commit: &str,
) -> anyhow::Result<()> {
    let records = store.integrations()?;
    let integrated: HashSet<_> = records
        .iter()
        .filter(|record| record.integrated_commit.is_some())
        .flat_map(|record| record.event_ids.iter().copied())
        .collect();
    let mut applicable = Vec::new();
    for event in store
        .events()?
        .into_iter()
        .filter(|event| event.workspace_id == workspace_id && !integrated.contains(&event.event_id))
    {
        // A remote merge object may not have been fetched yet. No filesystem or
        // remote write is needed; defer its membership proof until it is local.
        if GitService::new()
            .is_ancestor(root, &event.source_commit, merge_commit)
            .unwrap_or(false)
        {
            applicable.push(event);
        }
    }
    if applicable.is_empty() {
        return Ok(());
    }
    let mut record = records
        .into_iter()
        .find(|record| record.id == integration_id)
        .unwrap_or_else(|| MemoryIntegration {
            id: integration_id,
            event_ids: Vec::new(),
            target_branch: target_branch.into(),
            before_commit: applicable[0].base_commit.clone(),
            integrated_commit: Some(merge_commit.into()),
            source_error: None,
        });
    if record.target_branch != target_branch
        || record.integrated_commit.as_deref() != Some(merge_commit)
    {
        bail!(
            "External source integration identity changed; inspect its PR record before retrying"
        );
    }
    record
        .event_ids
        .extend(applicable.into_iter().map(|event| event.event_id));
    store.save_integration(&record)?;
    Ok(())
}

/// The same completion/guard boundary is used by direct merge, push and PR
/// creation. Remote merges must not bypass canonical Wiki ownership or publish
/// source before its semantic outbox record is available.
pub async fn prepare_workspace_source(
    pool: &sqlx::SqlitePool,
    git: &GitService,
    repo: &Repo,
    workspace: &Workspace,
    root: &Path,
    target_branch: &str,
) -> anyhow::Result<Option<String>> {
    let Some(store) = RepositoryMemoryStore::existing_for_repository(&repo.name, repo.id)? else {
        return Ok(None);
    };
    if !store.state()?.enabled {
        return Ok(None);
    }
    let base = git.get_base_commit(root, &workspace.branch, target_branch)?;
    guard_normal_workspace(git, root, &base.to_string())?;
    let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs WHERE workspace_id = ? AND status IN ('pending','starting','running','awaiting_input','awaiting_approval','cancelling')")
        .bind(workspace.id).fetch_one(pool).await?;
    if active > 0 {
        bail!("Finish the active coding run before integrating source and its semantic manifest");
    }
    let mut runs: Vec<_> = store
        .runs()?
        .into_iter()
        .filter(|run| run.workspace_id == workspace.id)
        .collect();
    runs.sort_by_key(|run| (run.created_at, run.run_id));
    let mut summary = None;
    for run in runs {
        if let Some(record) =
            db::models::agent_runtime::AgentRunRecord::find(pool, run.run_id).await?
            && record.status == executors::runtime::AgentRunStatus::Succeeded
            && let Some(event) = complete_coding_run(&store, &run)?
        {
            summary = Some(event.semantics.commit_message());
        }
    }
    Ok(summary)
}

/// Recover the Git/filesystem crash window through an explicit commit trailer,
/// never by guessing which original task commits survived a squash/rebase.
pub fn recover_integrations(store: &RepositoryMemoryStore, root: &Path) -> anyhow::Result<()> {
    let _lock = store.try_integration_lock()?;
    let integrations = store.integrations()?;
    let confirmed: HashSet<_> = integrations
        .iter()
        .filter(|record| record.integrated_commit.is_some())
        .flat_map(|record| record.event_ids.iter().copied())
        .collect();
    for mut record in integrations {
        if record.integrated_commit.is_some() {
            continue;
        }
        if !record.event_ids.is_empty() && record.event_ids.iter().all(|id| confirmed.contains(id))
        {
            record.source_error = Some("Superseded by a confirmed source integration".into());
            store.save_integration(&record)?;
            continue;
        }
        let range = format!("{}..{}", record.before_commit, record.target_branch);
        let marker = format!("EVK-Memory-Integration: {}", record.id);
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "log",
                "--format=%H",
                "--fixed-strings",
                "--grep",
                &marker,
                &range,
                "--",
            ])
            .output()?;
        if !output.status.success() {
            bail!("Cannot inspect source integration history; keep its events pending");
        }
        let text = String::from_utf8(output.stdout)?;
        let commits: Vec<_> = text.lines().collect();
        if commits.len() > 1 {
            bail!("Ambiguous source integration identity {}", record.id);
        }
        if let Some(commit) = commits.first() {
            record.integrated_commit = Some((*commit).into());
            record.source_error = None;
            store.save_integration(&record)?;
        } else if GitService::new().get_branch_oid(root, &record.target_branch)?
            == record.before_commit
        {
            // No integrated change exists at the checkpoint. The source merge
            // can be retried; an empty intent must not fence Wiki forever.
            record.source_error =
                Some("Source integration did not commit; retry the source merge".into());
            store.save_integration(&record)?;
        }
    }
    Ok(())
}

pub fn unresolved_integration(store: &RepositoryMemoryStore) -> anyhow::Result<bool> {
    Ok(store
        .integrations()?
        .iter()
        .any(|record| record.integrated_commit.is_none() && record.source_error.is_none()))
}

/// Selection uses explicit integration records, not original commit ancestry
/// (EVK's direct merge is a squash). Failed receipts remain pending.
pub fn pending_events(
    store: &RepositoryMemoryStore,
    target_branch: &str,
) -> anyhow::Result<Vec<ChangeManifest>> {
    let integrated: HashSet<_> = store
        .integrations()?
        .into_iter()
        .filter(|record| {
            record.target_branch == target_branch && record.integrated_commit.is_some()
        })
        .flat_map(|record| record.event_ids)
        .collect();
    store
        .events()?
        .into_iter()
        .filter(|event| integrated.contains(&event.event_id))
        .filter_map(|event| match store.receipt(event.event_id) {
            Ok(Some(receipt)) if receipt.result != ReconciliationResult::Failed => None,
            Ok(_) => Some(Ok(event)),
            Err(error) => Some(Err(error.into())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use utils::repository_memory::{SemanticChanges, WikiReconciliationReceipt};

    use super::*;

    fn git_command(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn repository(root: &Path) {
        std::fs::create_dir_all(root.join("openwiki")).unwrap();
        git_command(root, &["init", "-b", "main"]);
        git_command(root, &["config", "user.email", "test@example.invalid"]);
        git_command(root, &["config", "user.name", "EVK test"]);
        std::fs::write(root.join("source.txt"), "original source\n").unwrap();
        std::fs::write(root.join("openwiki/index.md"), "canonical snapshot\n").unwrap();
        git_command(root, &["add", "."]);
        git_command(root, &["commit", "-m", "base"]);
    }

    fn run(store: &RepositoryMemoryStore, root: &Path) -> RepositoryMemoryRun {
        let run_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        RepositoryMemoryRun {
            run_id,
            repository_id: Uuid::new_v4(),
            workspace_id,
            task_id: None,
            created_at: chrono::Utc::now(),
            base_commit: GitService::new().get_head_info(root).unwrap().oid,
            target_branch: "main".into(),
            repository_path: root.into(),
            memory_path: store.memory_path(workspace_id),
            draft_path: store.draft_path(run_id),
            wiki_status: utils::repository_memory::RepositoryWikiStatus::Current,
            finalize_source: true,
        }
    }

    #[test]
    fn two_worktrees_emit_independently_then_squash_batch_against_integrated_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        let a_path = temp.path().join("a");
        let b_path = temp.path().join("b");
        git_command(
            &root,
            &["worktree", "add", "-b", "a", a_path.to_str().unwrap()],
        );
        git_command(
            &root,
            &["worktree", "add", "-b", "b", b_path.to_str().unwrap()],
        );
        let a = run(&store, &a_path);
        let b = run(&store, &b_path);
        let before = a.base_commit.clone();
        for (run, name) in [(&a, "a.txt"), (&b, "b.txt")] {
            std::fs::write(run.repository_path.join(name), name).unwrap();
            std::fs::write(
                &run.draft_path,
                serde_json::to_vec(&SemanticChanges {
                    summary: format!("feat: add {name}"),
                    goal: "parallel source changes".into(),
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();
        }
        std::thread::scope(|scope| {
            scope.spawn(|| complete_coding_run(&store, &a).unwrap());
            scope.spawn(|| complete_coding_run(&store, &b).unwrap());
        });
        assert_eq!(store.events().unwrap().len(), 2);
        assert!(pending_events(&store, "main").unwrap().is_empty());
        for (run, branch) in [(&a, "a"), (&b, "b")] {
            assert_eq!(
                std::fs::read_to_string(run.repository_path.join("openwiki/index.md")).unwrap(),
                "canonical snapshot\n"
            );
            git_command(&run.repository_path, &["rebase", "main"]);
            let mut integration = prepare_integration(
                &store,
                run.workspace_id,
                "main",
                GitService::new().get_head_info(&root).unwrap().oid,
            )
            .unwrap();
            integration.integrated_commit = Some(
                GitService::new()
                    .merge_changes(
                        &root,
                        &run.repository_path,
                        branch,
                        "main",
                        &integration_commit_message("integrated source", integration.id),
                    )
                    .unwrap(),
            );
            if branch == "a" {
                // Simulate a crash after Git commit, before outbox confirmation.
                assert!(unresolved_integration(&store).unwrap());
                recover_integrations(&store, &root).unwrap();
                assert!(!unresolved_integration(&store).unwrap());
            } else {
                store.save_integration(&integration).unwrap();
            }
        }
        let events = pending_events(&store, "main").unwrap();
        assert_eq!(events.len(), 2);
        assert!(root.join("a.txt").exists() && root.join("b.txt").exists());
        let integrated = GitService::new().get_head_info(&root).unwrap().oid;
        assert_ne!(integrated, before);
        let mut receipt = WikiReconciliationReceipt {
            event_id: a.run_id,
            reconciled_at: chrono::Utc::now(),
            target_commit: integrated,
            wiki_commit: None,
            result: ReconciliationResult::Failed,
            error: Some("fixture provider failure".into()),
        };
        store.acknowledge(&receipt).unwrap();
        assert_eq!(pending_events(&store, "main").unwrap().len(), 2);
        receipt.result = ReconciliationResult::NoOp;
        receipt.error = None;
        store.acknowledge(&receipt).unwrap();
        assert_eq!(pending_events(&store, "main").unwrap().len(), 1);
        // Event replay never re-commits the now-integrated task branch.
        assert_eq!(
            complete_coding_run(&store, &a).unwrap().unwrap().event_id,
            a.run_id
        );
    }

    #[test]
    fn parallel_cards_reconcile_sequentially_from_the_latest_canonical_wiki() {
        use super::super::openwiki::{WikiPublicationRequest, publish_validated_wiki};

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        std::fs::write(
            root.join("openwiki/INSTRUCTIONS.md"),
            "Keep user instructions.\n",
        )
        .unwrap();
        git_command(&root, &["add", "openwiki/INSTRUCTIONS.md"]);
        git_command(&root, &["commit", "-m", "user Wiki instructions"]);
        let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
        let git = GitService::new();
        let mut runs = Vec::new();
        for name in ["a", "b"] {
            let path = temp.path().join(name);
            git_command(
                &root,
                &["worktree", "add", "-b", name, path.to_str().unwrap()],
            );
            let context = run(&store, &path);
            std::fs::write(path.join(format!("{name}.txt")), name).unwrap();
            std::fs::write(
                &context.draft_path,
                serde_json::to_vec(&SemanticChanges {
                    summary: format!("feat: {name}"),
                    ..Default::default()
                })
                .unwrap(),
            )
            .unwrap();
            runs.push((name, context));
        }
        std::thread::scope(|scope| {
            for (_, context) in &runs {
                let store = &store;
                scope.spawn(move || complete_coding_run(store, context).unwrap());
            }
        });
        let original_b = store.event(runs[1].1.run_id).unwrap().unwrap();
        let mut expected_wiki = "canonical snapshot\n".to_string();
        for (name, context) in runs {
            // B started before A's source and Wiki publication. Rebasing brings
            // both upstream artifacts together without branch-local Wiki edits.
            assert_eq!(
                std::fs::read_to_string(context.repository_path.join("openwiki/index.md")).unwrap(),
                "canonical snapshot\n"
            );
            git_command(&context.repository_path, &["rebase", "main"]);
            assert_eq!(
                std::fs::read_to_string(context.repository_path.join("openwiki/index.md")).unwrap(),
                expected_wiki
            );
            let mut integration = prepare_integration(
                &store,
                context.workspace_id,
                "main",
                git.get_head_info(&root).unwrap().oid,
            )
            .unwrap();
            assert_eq!(integration.event_ids, vec![context.run_id]);
            let source = git
                .merge_changes(
                    &root,
                    &context.repository_path,
                    name,
                    "main",
                    &integration_commit_message(&format!("feat: {name}"), integration.id),
                )
                .unwrap();
            integration.integrated_commit = Some(source.clone());
            store.save_integration(&integration).unwrap();
            let events = pending_events(&store, "main").unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].event_id, context.run_id);
            if name == "b" {
                assert!(
                    !git.is_ancestor(&root, &original_b.source_commit, &source)
                        .unwrap()
                );
            }

            let maintenance_branch = format!("wiki-{name}");
            let maintenance = temp.path().join(&maintenance_branch);
            git_command(
                &root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &maintenance_branch,
                    maintenance.to_str().unwrap(),
                    "main",
                ],
            );
            assert_eq!(git.get_head_info(&maintenance).unwrap().oid, source);
            assert_eq!(
                std::fs::read_to_string(maintenance.join("openwiki/index.md")).unwrap(),
                expected_wiki
            );
            assert!(maintenance.join("a.txt").is_file());
            assert_eq!(maintenance.join("b.txt").is_file(), name == "b");

            // Publication fixture supplies a validated output; registered MCP/all-attempt
            // proof is covered through the production gate in server completion tests.
            expected_wiki = format!("Validated Wiki after {name}\n");
            std::fs::write(maintenance.join("openwiki/index.md"), &expected_wiki).unwrap();
            let (wiki_commit, no_op) = publish_validated_wiki(
                &git,
                &store,
                &WikiPublicationRequest {
                    repository_root: &root,
                    maintenance_root: &maintenance,
                    maintenance_branch: &maintenance_branch,
                    target_branch: "main",
                    source_commit: &source,
                    run_id: Uuid::new_v4(),
                },
            )
            .unwrap();
            assert!(!no_op);
            store
                .acknowledge(&WikiReconciliationReceipt {
                    event_id: context.run_id,
                    reconciled_at: chrono::Utc::now(),
                    target_commit: source.clone(),
                    wiki_commit: wiki_commit.clone(),
                    result: ReconciliationResult::Updated,
                    error: None,
                })
                .unwrap();
            assert!(pending_events(&store, "main").unwrap().is_empty());
            assert_eq!(git.get_head_info(&root).unwrap().oid, wiki_commit.unwrap());
            assert_eq!(git_command(&root, &["rev-parse", "HEAD^"]), source);
            assert_eq!(
                std::fs::read_to_string(root.join("openwiki/index.md")).unwrap(),
                expected_wiki
            );
            assert_eq!(
                std::fs::read_to_string(root.join("openwiki/INSTRUCTIONS.md")).unwrap(),
                "Keep user instructions.\n"
            );
        }
        assert_eq!(store.events().unwrap().len(), 2);
    }

    #[test]
    fn control_run_does_not_commit_existing_dirty_source_or_require_a_draft() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        let mut context = run(&store, &root);
        context.finalize_source = false;
        std::fs::write(root.join("source.txt"), "uncommitted work").unwrap();
        assert!(complete_coding_run(&store, &context).unwrap().is_none());
        assert_eq!(
            git_command(&root, &["rev-parse", "HEAD"]),
            context.base_commit
        );
        assert!(!git_command(&root, &["status", "--porcelain"]).is_empty());
        assert!(store.events().unwrap().is_empty());
    }

    #[test]
    fn missing_draft_is_visible_retryable_and_direct_folder_source_is_integrated_first() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        let context = run(&store, &root);
        std::fs::write(root.join("source.txt"), "updated direct-folder source").unwrap();
        assert!(complete_coding_run(&store, &context).is_err());
        assert_eq!(store.coding_errors().unwrap().len(), 1);
        assert_eq!(
            git_command(&root, &["rev-parse", "HEAD"]),
            context.base_commit
        );
        std::fs::write(
            &context.draft_path,
            serde_json::to_vec(&SemanticChanges {
                summary: "fix source".into(),
                goal: "test retry".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let event = complete_coding_run(&store, &context).unwrap().unwrap();
        assert!(store.coding_errors().unwrap().is_empty());
        assert_eq!(
            pending_events(&store, "main").unwrap()[0].event_id,
            event.event_id
        );
        assert_eq!(
            git_command(&root, &["rev-parse", "HEAD"]),
            event.source_commit
        );
        assert_eq!(
            git_command(&root, &["log", "-1", "--format=%s"]),
            event.semantics.commit_message()
        );
    }

    #[test]
    fn direct_folder_on_target_branch_cannot_hide_committed_wiki_edits() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        let context = run(&store, &root);
        std::fs::write(root.join("openwiki/index.md"), "unexpected edit").unwrap();
        git_command(&root, &["commit", "-am", "unexpected wiki change"]);
        assert!(complete_coding_run(&store, &context).is_err());
        assert!(store.events().unwrap().is_empty());
    }

    #[test]
    fn parallel_workspace_rebases_published_wiki_without_claiming_upstream_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        let b_path = temp.path().join("b");
        git_command(
            &root,
            &["worktree", "add", "-b", "b", b_path.to_str().unwrap()],
        );
        let context = run(&store, &b_path);
        // A has integrated and the single writer published a newer Wiki.
        std::fs::write(root.join("a.txt"), "integrated A").unwrap();
        git_command(&root, &["add", "a.txt"]);
        git_command(&root, &["commit", "-m", "A source"]);
        std::fs::write(root.join("openwiki/index.md"), "Wiki after A\n").unwrap();
        git_command(&root, &["commit", "-am", "Wiki after integrated A"]);
        assert_eq!(
            std::fs::read_to_string(b_path.join("openwiki/index.md")).unwrap(),
            "canonical snapshot\n"
        );
        std::fs::write(b_path.join("b.txt"), "source B").unwrap();
        git_command(&b_path, &["add", "b.txt"]);
        git_command(&b_path, &["commit", "-m", "B source"]);
        git_command(&b_path, &["rebase", "main"]);
        std::fs::write(
            &context.draft_path,
            serde_json::to_vec(&SemanticChanges {
                summary: "B source".into(),
                goal: "B".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let event = complete_coding_run(&store, &context).unwrap().unwrap();
        assert_eq!(event.changed_paths, vec!["b.txt"]);
        assert_eq!(
            std::fs::read_to_string(b_path.join("openwiki/index.md")).unwrap(),
            "Wiki after A\n"
        );
        let mut integration = prepare_integration(
            &store,
            context.workspace_id,
            "main",
            git_command(&root, &["rev-parse", "HEAD"]),
        )
        .unwrap();
        integration.integrated_commit = Some(
            GitService::new()
                .merge_changes(
                    &root,
                    &b_path,
                    "b",
                    "main",
                    &integration_commit_message("B source", integration.id),
                )
                .unwrap(),
        );
        store.save_integration(&integration).unwrap();
        let pending = pending_events(&store, "main").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].event_id, event.event_id);
        assert!(root.join("a.txt").is_file() && root.join("b.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("openwiki/index.md")).unwrap(),
            "Wiki after A\n"
        );
    }

    #[test]
    fn no_change_checkpoint_does_not_capture_a_later_task_and_wiki_rename_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
        let a = run(&store, &root);
        assert!(complete_coding_run(&store, &a).unwrap().is_none());
        std::fs::write(root.join("next-task.txt"), "not the previous run").unwrap();
        assert!(complete_coding_run(&store, &a).unwrap().is_none());
        let b = run(&store, &root);
        git_command(&root, &["mv", "openwiki/index.md", "moved.md"]);
        assert!(
            complete_coding_run(&store, &b)
                .unwrap_err()
                .to_string()
                .contains("canonical openwiki")
        );
        assert_eq!(
            GitService::new().get_head_info(&root).unwrap().oid,
            b.base_commit
        );
        assert!(root.join("moved.md").exists());
    }
    #[test]
    fn source_commit_outbox_crash_replays_frozen_semantics_not_a_later_task() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        git_command(&root, &["checkout", "-b", "feature"]);
        let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
        let context = run(&store, &root);
        std::fs::write(root.join("source.txt"), "first task\n").unwrap();
        let publication = SourcePublication {
            manifest: ChangeManifest {
                version: 1,
                event_id: context.run_id,
                repository_id: context.repository_id,
                workspace_id: context.workspace_id,
                task_id: context.task_id,
                created_at: context.created_at,
                base_commit: context.base_commit.clone(),
                source_commit: context.base_commit.clone(),
                target_branch: Some("main".into()),
                changed_paths: vec!["source.txt".into()],
                semantics: SemanticChanges {
                    goal: "first task only".into(),
                    summary: "fix: first task".into(),
                    ..Default::default()
                },
            },
            staged_tree: GitService::new().stage_source_snapshot(&root).unwrap(),
        };
        store.save_source_publication(&publication).unwrap();
        GitService::new()
            .commit_staged_snapshot(
                &root,
                &format!("fix: first task\n\nEVK-Memory-Source: {}", context.run_id),
                &context.base_commit,
                &publication.staged_tree,
            )
            .unwrap();
        let first_commit = git_command(&root, &["rev-parse", "HEAD"]);
        // Server died after Git, before publishing its event. Another task then
        // committed and the mutable draft was overwritten. Recovery must not
        // derive either source or semantics from that new task.
        std::fs::write(root.join("second.txt"), "second task\n").unwrap();
        git_command(&root, &["add", "second.txt"]);
        git_command(&root, &["commit", "-m", "second task"]);
        let second_commit = git_command(&root, &["rev-parse", "HEAD"]);
        std::fs::write(&context.draft_path, "not even valid JSON").unwrap();
        let event = complete_coding_run(&store, &context).unwrap().unwrap();
        assert_eq!(event.source_commit, first_commit);
        assert_eq!(event.changed_paths, vec!["source.txt"]);
        assert_eq!(event.semantics.summary, "fix: first task");
        assert_eq!(event.created_at, context.created_at);
        assert_eq!(git_command(&root, &["rev-parse", "HEAD"]), second_commit);
        assert_eq!(complete_coding_run(&store, &context).unwrap(), Some(event));
        assert_eq!(store.events().unwrap().len(), 1);
    }

    #[test]
    fn source_publication_retry_commits_only_frozen_index_and_keeps_later_edits() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        git_command(&root, &["checkout", "-b", "feature"]);
        let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
        let context = run(&store, &root);
        std::fs::write(root.join("source.txt"), "frozen result\n").unwrap();
        let publication = SourcePublication {
            manifest: ChangeManifest {
                version: 1,
                event_id: context.run_id,
                repository_id: context.repository_id,
                workspace_id: context.workspace_id,
                task_id: context.task_id,
                created_at: context.created_at,
                base_commit: context.base_commit.clone(),
                source_commit: context.base_commit.clone(),
                target_branch: Some("main".into()),
                changed_paths: vec!["source.txt".into()],
                semantics: SemanticChanges {
                    summary: "fix: frozen result".into(),
                    ..Default::default()
                },
            },
            staged_tree: GitService::new().stage_source_snapshot(&root).unwrap(),
        };
        store.save_source_publication(&publication).unwrap();
        // Crash before Git commit; later unstaged changes are deliberately not
        // swept into the completion commit when the checkpoint is replayed.
        std::fs::write(root.join("source.txt"), "later unstaged edit\n").unwrap();
        std::fs::write(root.join("next.txt"), "later untracked edit\n").unwrap();
        let event = complete_coding_run(&store, &context).unwrap().unwrap();
        assert_eq!(
            git_command(
                &root,
                &["show", &format!("{}:source.txt", event.source_commit)]
            ),
            "frozen result"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("source.txt")).unwrap(),
            "later unstaged edit\n"
        );
        assert!(root.join("next.txt").is_file());
        assert!(!git_command(&root, &["status", "--porcelain"]).is_empty());
    }

    #[tokio::test]
    async fn follow_up_recovers_prior_completion_before_new_source_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        git_command(&root, &["checkout", "-b", "feature"]);
        let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE agent_runs (id BLOB PRIMARY KEY, status TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        let first = run(&store, &root);
        let unrelated_workspace = run(&store, &root);
        for run in [&first, &unrelated_workspace] {
            store.save_run(run).unwrap();
            sqlx::query("INSERT INTO agent_runs VALUES (?, 'succeeded')")
                .bind(run.run_id)
                .execute(&pool)
                .await
                .unwrap();
        }
        std::fs::write(root.join("first.txt"), "first result").unwrap();
        let next = Uuid::new_v4();
        // Missing semantic output is explicit and fail-closed, not silently
        // filled using a new coding turn's source or another workspace draft.
        let error = complete_previous_coding_runs(&pool, &store, first.workspace_id, next)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains(&first.run_id.to_string()));
        assert_eq!(
            git_command(&root, &["rev-parse", "HEAD"]),
            first.base_commit
        );
        std::fs::write(
            &first.draft_path,
            serde_json::to_vec(&SemanticChanges {
                summary: "feat: first".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        complete_previous_coding_runs(&pool, &store, first.workspace_id, next)
            .await
            .unwrap();
        let first_event = store.event(first.run_id).unwrap().unwrap();
        assert_eq!(first_event.changed_paths, vec!["first.txt"]);
        assert!(store.event(unrelated_workspace.run_id).unwrap().is_none());
        assert!(store.coding_errors().unwrap().is_empty());

        let mut second = run(&store, &root);
        second.run_id = next;
        second.workspace_id = first.workspace_id;
        second.draft_path = store.draft_path(next);
        store.save_run(&second).unwrap();
        sqlx::query("INSERT INTO agent_runs VALUES (?, 'succeeded')")
            .bind(next)
            .execute(&pool)
            .await
            .unwrap();
        std::fs::write(root.join("second.txt"), "second result").unwrap();
        std::fs::write(
            &second.draft_path,
            serde_json::to_vec(&SemanticChanges {
                summary: "feat: second".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        complete_previous_coding_runs(&pool, &store, first.workspace_id, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(store.event(first.run_id).unwrap(), Some(first_event));
        assert_eq!(
            store.event(next).unwrap().unwrap().changed_paths,
            vec!["second.txt"]
        );
        assert_eq!(store.events().unwrap().len(), 2);
    }

    #[test]
    fn wiki_guard_is_precise() {
        assert!(canonical_wiki_path("openwiki/a.md"));
        assert!(canonical_wiki_path("openwiki"));
        assert!(!canonical_wiki_path("src/openwiki.rs"));
        assert!(!canonical_wiki_path("openwiki-example/a.md"));
    }

    #[test]
    fn external_pr_never_acknowledges_unpushed_changes_by_workspace_or_time() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        repository(&root);
        let feature = temp.path().join("feature");
        git_command(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature.to_str().unwrap(),
            ],
        );
        let store = RepositoryMemoryStore::at_persistent(temp.path()).unwrap();
        let first = run(&store, &feature);
        std::fs::write(feature.join("first.txt"), "pushed first task").unwrap();
        std::fs::write(
            &first.draft_path,
            serde_json::to_vec(&SemanticChanges {
                summary: "feat: pushed first task".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let first_event = complete_coding_run(&store, &first).unwrap().unwrap();
        let mut second = run(&store, &feature);
        second.workspace_id = first.workspace_id;
        std::fs::write(feature.join("second.txt"), "unpushed second task").unwrap();
        std::fs::write(
            &second.draft_path,
            serde_json::to_vec(&SemanticChanges {
                summary: "feat: unpushed second task".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let second_event = complete_coding_run(&store, &second).unwrap().unwrap();
        // Both tasks were completed before the PR merged, in the same workspace.
        // Only the first commit actually belonged to the merged remote branch.
        git_command(
            &root,
            &[
                "merge",
                "--no-ff",
                &first_event.source_commit,
                "-m",
                "remote PR",
            ],
        );
        let merged = git_command(&root, &["rev-parse", "HEAD"]);
        let pr = Uuid::new_v4();
        observe_external_integration(&store, &root, pr, first.workspace_id, "main", &merged)
            .unwrap();
        assert_eq!(
            pending_events(&store, "main")
                .unwrap()
                .iter()
                .map(|event| event.event_id)
                .collect::<Vec<_>>(),
            vec![first_event.event_id]
        );
        assert!(store.receipt(second_event.event_id).unwrap().is_none());

        // A delayed outbox event for an already integrated commit is still
        // attributable: event creation time is not source integration time.
        let mut delayed = first_event.clone();
        delayed.event_id = Uuid::new_v4();
        delayed.created_at = chrono::Utc::now() + chrono::Duration::days(1);
        store.publish_event(&delayed).unwrap();
        observe_external_integration(&store, &root, pr, first.workspace_id, "main", &merged)
            .unwrap();
        assert_eq!(pending_events(&store, "main").unwrap().len(), 2);
        observe_external_integration(&store, &root, pr, first.workspace_id, "main", &merged)
            .unwrap();
        assert_eq!(store.integrations().unwrap()[0].event_ids.len(), 2);

        git_command(
            &root,
            &["checkout", "-b", "external-squash", &first.base_commit],
        );
        git_command(&root, &["merge", "--squash", &first_event.source_commit]);
        git_command(
            &root,
            &["commit", "-m", "external squash without EVK membership"],
        );
        let squash = git_command(&root, &["rev-parse", "HEAD"]);
        let mut unknown = first_event;
        unknown.event_id = Uuid::new_v4();
        unknown.workspace_id = Uuid::new_v4();
        store.publish_event(&unknown).unwrap();
        observe_external_integration(
            &store,
            &root,
            Uuid::new_v4(),
            unknown.workspace_id,
            "external-squash",
            &squash,
        )
        .unwrap();
        assert!(
            pending_events(&store, "external-squash")
                .unwrap()
                .is_empty()
        );
        assert!(store.receipt(unknown.event_id).unwrap().is_none());
    }
}
