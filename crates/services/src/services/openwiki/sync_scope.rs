//! Resolve adoption boundaries from immutable Git commits, never user patches.
use std::{path::Path, process::Command};

use anyhow::{Context, ensure};
use git::GitService;
use utils::repository_memory::{ChangeManifest, RepositoryWikiSyncScope};

pub fn resolve(
    root: &Path,
    source: &str,
    from_commit: Option<&str>,
) -> anyhow::Result<RepositoryWikiSyncScope> {
    let Some(input) = from_commit else {
        return Ok(RepositoryWikiSyncScope::CurrentSource);
    };
    let input = input.trim();
    ensure!(
        (4..=40).contains(&input.len()) && input.bytes().all(|b| b.is_ascii_hexdigit()),
        "Enter a commit SHA (4–40 hexadecimal characters)"
    );
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{input}^{{commit}}"),
        ])
        .output()?;
    ensure!(
        output.status.success(),
        "The starting commit is missing or ambiguous in this repository"
    );
    let base_commit = String::from_utf8(output.stdout)?.trim().to_owned();
    ensure!(
        GitService::new().is_ancestor(root, &base_commit, source)?,
        "The starting commit must be an ancestor of the selected local branch"
    );
    Ok(RepositoryWikiSyncScope::SinceCommit { base_commit })
}

pub fn select_events(
    root: &Path,
    scope: &RepositoryWikiSyncScope,
    events: Vec<ChangeManifest>,
) -> anyhow::Result<Vec<ChangeManifest>> {
    let RepositoryWikiSyncScope::SinceCommit { base_commit } = scope else {
        return Ok(events);
    };
    let git = GitService::new();
    events
        .into_iter()
        .filter_map(|event| {
            match git
                .is_ancestor(root, base_commit, &event.base_commit)
                .context("Cannot verify Change Manifest against the requested commit range")
            {
                Ok(true) => Some(Ok(event)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
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
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    #[test]
    fn adoption_scope_freezes_an_ancestor_and_rejects_unrelated_or_invalid_input() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "user.email", "test@example.invalid"]);
        git(root, &["commit", "--allow-empty", "-m", "base"]);
        let base = git(root, &["rev-parse", "HEAD"]);
        git(root, &["commit", "--allow-empty", "-m", "source"]);
        let source = git(root, &["rev-parse", "HEAD"]);
        assert_eq!(
            resolve(root, &source, Some(&base[..8])).unwrap(),
            RepositoryWikiSyncScope::SinceCommit {
                base_commit: base.clone()
            }
        );
        assert_eq!(
            resolve(root, &source, None).unwrap(),
            RepositoryWikiSyncScope::CurrentSource
        );
        assert!(resolve(root, &base, Some(&source)).is_err());
        for invalid in ["", "main", "--help", "HEAD~1", "deadbeef"] {
            assert!(resolve(root, &source, Some(invalid)).is_err());
        }

        // A selected range must not acknowledge an older event or one spanning
        // its lower boundary. Only wholly covered events become Sync inputs.
        git(root, &["commit", "--allow-empty", "-m", "head"]);
        let head = git(root, &["rev-parse", "HEAD"]);
        let event = |from: &str, to: &str| ChangeManifest {
            version: 1,
            repository_id: uuid::Uuid::new_v4(),
            workspace_id: uuid::Uuid::new_v4(),
            event_id: uuid::Uuid::new_v4(),
            task_id: None,
            created_at: chrono::Utc::now(),
            base_commit: from.into(),
            source_commit: to.into(),
            target_branch: Some("main".into()),
            changed_paths: vec!["src/app.rs".into()],
            semantics: Default::default(),
        };
        let covered = event(&source, &head);
        let selected = select_events(
            root,
            &RepositoryWikiSyncScope::SinceCommit {
                base_commit: source.clone(),
            },
            vec![event(&base, &source), event(&base, &head), covered.clone()],
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].event_id, covered.event_id);

        git(root, &["checkout", "--orphan", "unrelated"]);
        git(root, &["commit", "--allow-empty", "-m", "unrelated"]);
        let unrelated = git(root, &["rev-parse", "HEAD"]);
        assert!(resolve(root, &head, Some(&unrelated)).is_err());
    }
}
