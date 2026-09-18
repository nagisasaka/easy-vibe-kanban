//! Exact, host-validated local promotion. This is deliberately separate from
//! manual squash merge: no new commit, source ref rewrite, archive or Card update.
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use git2::{Oid, Reference, Repository, RepositoryState, StatusOptions};

use crate::{GitCli, GitService, GitServiceError};

#[derive(Debug)]
pub struct GitPublicationGuard(File);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactPublicationState {
    Applied,
    NotApplied,
}

/// `read-tree -m -u` may overwrite ignored, untracked files. They are allowed
/// build products only when they do not collide with a path in R. Inspect names
/// and metadata (never follow a local symlink) before changing any files/index.
fn guard_checkout_collisions(
    repo: &Repository,
    root: &Path,
    base: Oid,
    result: Oid,
) -> Result<(), GitServiceError> {
    let before = repo.find_commit(base)?.tree()?;
    let after = repo.find_commit(result)?.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&before), Some(&after), None)?;
    for delta in diff.deltas() {
        let Some(path) = delta.new_file().path() else {
            continue;
        };
        if after.get_path(path).is_err() {
            continue;
        } // deletion
        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component);
            let metadata = match std::fs::symlink_metadata(root.join(&prefix)) {
                Ok(metadata) => metadata,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let tracked_file = before
                .get_path(&prefix)
                .is_ok_and(|entry| matches!(entry.filemode(), 0o100644 | 0o100755 | 0o120000));
            let existing_parent_directory =
                prefix != path && metadata.is_dir() && !metadata.file_type().is_symlink();
            if !tracked_file && !existing_parent_directory {
                return Err(GitServiceError::WorktreeDirty(
                    root.display().to_string(),
                    format!(
                        "Target contains local data at {} that R would replace (including ignored paths)",
                        prefix.display()
                    ),
                ));
            }
        }
    }
    Ok(())
}

impl Drop for GitPublicationGuard {
    fn drop(&mut self) {
        if let Err(error) = self.0.unlock() {
            tracing::warn!(%error, "Could not explicitly release Git publication guard");
        }
    }
}

impl GitService {
    /// Reconcile a durable caller intent without replaying Git writes. A prior
    /// confirmed receipt permits later target changes/dirty user edits; without
    /// that receipt, every target checkout must still prove the observed tree.
    pub fn inspect_exact_publication(
        &self,
        root: &Path,
        target_ref: &str,
        base: &str,
        result: &str,
        confirmed: bool,
        managed_worktrees: &[PathBuf],
    ) -> Result<ExactPublicationState, GitServiceError> {
        let _guard = self.try_publication_guard(root)?;
        if !target_ref.starts_with("refs/heads/") || !Reference::is_valid_name(target_ref) {
            return Err(GitServiceError::InvalidRepository(
                "Invalid local target identity".into(),
            ));
        }
        let repo = self.open_repo(root)?;
        let head = repo.find_reference(target_ref)?.peel_to_commit()?.id();
        let base = Oid::from_str(base)?;
        let result = Oid::from_str(result)?;
        let state = if head == result || (confirmed && repo.graph_descendant_of(head, result)?) {
            ExactPublicationState::Applied
        } else if head == base && !confirmed {
            ExactPublicationState::NotApplied
        } else {
            return Err(GitServiceError::InvalidRepository("Publication is unproven: target is neither the recorded result nor an unchanged base; no reset/remerge attempted".into()));
        };
        if !confirmed {
            for entry in GitCli::new().list_worktrees(root)? {
                if entry
                    .branch
                    .as_deref()
                    .is_some_and(|b| b == target_ref || b == &target_ref[11..])
                {
                    let path = std::fs::canonicalize(entry.path)?;
                    if !managed_worktrees
                        .iter()
                        .any(|p| std::fs::canonicalize(p).is_ok_and(|p| p == path))
                    {
                        return Err(GitServiceError::InvalidRepository(
                            "Cannot verify an unmanaged target checkout".into(),
                        ));
                    }
                    self.require_clean_source(&path, &head.to_string())?;
                }
            }
        }
        Ok(state)
    }
    pub fn storage_identity(&self, root: &Path) -> Result<PathBuf, GitServiceError> {
        Ok(std::fs::canonicalize(self.get_common_dir(root)?)?)
    }

    /// Storage-scoped, independent of registered repo ID or Memory enablement.
    /// Do not hold across model execution. This does not lock external Git users.
    pub fn try_publication_guard(
        &self,
        root: &Path,
    ) -> Result<GitPublicationGuard, GitServiceError> {
        let path = self.storage_identity(root)?.join("evk-publication.lock");
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(GitServiceError::InvalidRepository(
                "Invalid publication lock path".into(),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock()
            .map_err(|error| GitServiceError::IoError(error.into()))?;
        Ok(GitPublicationGuard(file))
    }

    pub fn create_branch_at_commit(
        &self,
        root: &Path,
        name: &str,
        oid: &str,
    ) -> Result<(), GitServiceError> {
        let repo = self.open_repo(root)?;
        let commit = repo.find_commit(Oid::from_str(oid)?)?;
        repo.branch(name, &commit, false)?;
        Ok(())
    }

    /// Ignored build products are allowed; tracked edits, untracked source and
    /// unfinished Git operations are not. No stash/reset or side-effecting setup.
    pub fn require_clean_source(&self, root: &Path, expected: &str) -> Result<(), GitServiceError> {
        let repo = self.open_repo(root)?;
        if repo.head()?.peel_to_commit()?.id().to_string() != expected {
            return Err(GitServiceError::InvalidRepository(
                "SOURCE_CHANGED: HEAD differs from the frozen commit".into(),
            ));
        }
        if repo.state() != RepositoryState::Clean {
            return Err(GitServiceError::WorktreeDirty(
                root.display().to_string(),
                "unfinished Git operation".into(),
            ));
        }
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        if !repo.statuses(Some(&mut options))?.is_empty() {
            return Err(GitServiceError::WorktreeDirty(
                root.display().to_string(),
                "tracked or untracked changes present".into(),
            ));
        }
        Ok(())
    }

    pub fn promote_exact(
        &self,
        root: &Path,
        target_ref: &str,
        expected_base: &str,
        verified_result: &str,
        managed_worktrees: &[PathBuf],
        run_id: &str,
    ) -> Result<(), GitServiceError> {
        let _guard = self.try_publication_guard(root)?;
        self.promote_exact_inner(
            root,
            target_ref,
            expected_base,
            verified_result,
            managed_worktrees,
            run_id,
            || Ok(()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn promote_exact_inner(
        &self,
        root: &Path,
        target_ref: &str,
        expected_base: &str,
        verified_result: &str,
        managed_worktrees: &[PathBuf],
        run_id: &str,
        after_checkout: impl FnOnce() -> Result<(), GitServiceError>,
    ) -> Result<(), GitServiceError> {
        if !target_ref.starts_with("refs/heads/")
            || !Reference::is_valid_name(target_ref)
            || run_id.contains('\0')
        {
            return Err(GitServiceError::InvalidRepository(
                "Expected a valid full local target ref".into(),
            ));
        }
        let repo = self.open_repo(root)?;
        let base = Oid::from_str(expected_base)?;
        let result = Oid::from_str(verified_result)?;
        repo.find_commit(base)?;
        repo.find_commit(result)?;
        if base != result && !repo.graph_descendant_of(result, base)? {
            return Err(GitServiceError::BranchesDiverged(
                "Verified R does not contain B".into(),
            ));
        }
        // Lock the one ref, then compare B. An earlier ff-only preflight is not
        // a CAS. The transaction never creates an unvalidated merge commit.
        let mut transaction = repo.transaction()?;
        transaction.lock_ref(target_ref)?;
        if repo.find_reference(target_ref)?.peel_to_commit()?.id() != base {
            return Err(GitServiceError::InvalidRepository(
                "TARGET_CHANGED: target no longer equals B".into(),
            ));
        }
        let branch = target_ref.trim_start_matches("refs/heads/");
        let mut checked_out = Vec::new();
        for entry in GitCli::new().list_worktrees(root)? {
            if entry
                .branch
                .as_deref()
                .is_some_and(|value| value == branch || value == target_ref)
            {
                let path = std::fs::canonicalize(entry.path)?;
                if !managed_worktrees.iter().any(|allowed| {
                    std::fs::canonicalize(allowed).is_ok_and(|allowed| allowed == path)
                }) {
                    return Err(GitServiceError::WorktreeDirty(
                        path.display().to_string(),
                        "TARGET_WORKTREE_BUSY: target checkout is not managed by this operation"
                            .into(),
                    ));
                }
                self.require_clean_source(&path, expected_base)?;
                guard_checkout_collisions(&repo, &path, base, result)?;
                checked_out.push(path);
            }
        }
        if checked_out.len() > 1 {
            return Err(GitServiceError::InvalidRepository(
                "Target is checked out more than once; publication refused".into(),
            ));
        }
        for path in &checked_out {
            let checkout = self.open_repo(path)?;
            if checkout.head()?.name() != Some(target_ref) {
                return Err(GitServiceError::InvalidRepository(
                    "Target checkout switched branches".into(),
                ));
            }
            // The existing CLI boundary updates files AND index. A crash before ref commit
            // can leave B with R's files: durable caller intent must detect that
            // and require recovery, never reset user edits or claim success.
            GitCli::new().checkout_exact_trees(path, expected_base, verified_result)?;
        }
        after_checkout()?;
        transaction.set_target(
            target_ref,
            result,
            Some(&self.signature_with_fallback(&repo)?),
            &format!("EVK Integration {run_id}: verified exact result"),
        )?;
        transaction.commit()?;
        if repo.find_reference(target_ref)?.peel_to_commit()?.id() != result {
            return Err(GitServiceError::InvalidRepository(
                "Publication result changed; recovery required".into(),
            ));
        }
        for path in &checked_out {
            self.require_clean_source(path, verified_result)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn git(root: &Path, args: &[&str]) -> String {
        let result = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().into()
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        std::fs::write(root.join("source"), "base").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        git(&root, &["checkout", "-b", "result"]);
        std::fs::write(root.join("source"), "verified").unwrap();
        git(&root, &["commit", "-am", "result"]);
        let result = git(&root, &["rev-parse", "HEAD"]);
        git(&root, &["checkout", "main"]);
        (temp, root, base, result)
    }
    #[test]
    fn exact_publication_preserves_source_and_updates_checked_out_target() {
        let (_temp, root, base, result) = fixture();
        GitService::new()
            .promote_exact(
                &root,
                "refs/heads/main",
                &base,
                &result,
                std::slice::from_ref(&root),
                "test",
            )
            .unwrap();
        assert_eq!(git(&root, &["rev-parse", "main"]), result);
        assert_eq!(git(&root, &["rev-parse", "result"]), result);
        assert!(git(&root, &["status", "--porcelain"]).is_empty());
        assert_eq!(
            std::fs::read_to_string(root.join("source")).unwrap(),
            "verified"
        );
        assert_eq!(git(&root, &["rev-list", "--count", "main"]), "2");
    }
    #[test]
    fn stale_base_dirty_untracked_unmanaged_and_wrong_ref_are_refused() {
        let (_temp, root, base, result) = fixture();
        let service = GitService::new();
        assert!(
            service
                .promote_exact(
                    &root,
                    "refs/heads/main",
                    &result,
                    &result,
                    std::slice::from_ref(&root),
                    "test"
                )
                .is_err()
        );
        assert!(
            service
                .promote_exact(&root, "refs/heads/main", &base, &result, &[], "test")
                .is_err()
        );
        assert!(
            service
                .promote_exact(
                    &root,
                    "refs/remotes/origin/main",
                    &base,
                    &result,
                    std::slice::from_ref(&root),
                    "test"
                )
                .is_err()
        );
        std::fs::write(root.join("user-file"), "keep").unwrap();
        assert!(
            service
                .promote_exact(
                    &root,
                    "refs/heads/main",
                    &base,
                    &result,
                    std::slice::from_ref(&root),
                    "test"
                )
                .is_err()
        );
        assert_eq!(git(&root, &["rev-parse", "main"]), base);
        assert_eq!(
            std::fs::read_to_string(root.join("user-file")).unwrap(),
            "keep"
        );
    }
    #[test]
    fn no_checkout_noop_and_shared_storage_lock() {
        let (temp, root, base, result) = fixture();
        git(&root, &["checkout", "result"]);
        let service = GitService::new();
        service
            .promote_exact(&root, "refs/heads/main", &base, &result, &[], "test")
            .unwrap();
        service
            .promote_exact(&root, "refs/heads/main", &result, &result, &[], "test")
            .unwrap();
        let peer = temp.path().join("peer");
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "peer",
                peer.to_str().unwrap(),
                "main",
            ],
        );
        let guard = service.try_publication_guard(&root).unwrap();
        assert_eq!(
            service.storage_identity(&root).unwrap(),
            service.storage_identity(&peer).unwrap()
        );
        assert!(service.try_publication_guard(&peer).is_err());
        drop(guard);
        service.try_publication_guard(&peer).unwrap();
    }
    #[test]
    fn checkout_failure_window_is_observable_without_destructive_repair() {
        let (_temp, root, base, result) = fixture();
        let service = GitService::new();
        let error = service.promote_exact_inner(
            &root,
            "refs/heads/main",
            &base,
            &result,
            std::slice::from_ref(&root),
            "crash",
            || {
                Err(GitServiceError::IoError(std::io::Error::other(
                    "injected after checkout",
                )))
            },
        );
        assert!(error.is_err());
        assert_eq!(git(&root, &["rev-parse", "main"]), base);
        assert_eq!(
            std::fs::read_to_string(root.join("source")).unwrap(),
            "verified"
        );
        assert!(service.require_clean_source(&root, &base).is_err());
        // No automatic retry over the half-published files, and no reset.
        assert!(
            service
                .promote_exact(
                    &root,
                    "refs/heads/main",
                    &base,
                    &result,
                    std::slice::from_ref(&root),
                    "crash"
                )
                .is_err()
        );
    }

    #[test]
    fn ignored_local_data_colliding_with_result_is_preserved() {
        let (_temp, root, base, _) = fixture();
        git(&root, &["checkout", "result"]);
        std::fs::write(root.join("cache"), "new tracked source").unwrap();
        git(&root, &["add", "cache"]);
        git(&root, &["commit", "-m", "add tracked path"]);
        let result = git(&root, &["rev-parse", "HEAD"]);
        git(&root, &["checkout", "main"]);
        std::fs::write(root.join(".git/info/exclude"), "cache\n").unwrap();
        std::fs::write(root.join("cache"), "user data, not disposable").unwrap();
        let service = GitService::new();
        service.require_clean_source(&root, &base).unwrap();
        assert!(
            service
                .promote_exact(
                    &root,
                    "refs/heads/main",
                    &base,
                    &result,
                    std::slice::from_ref(&root),
                    "ignored-collision"
                )
                .is_err()
        );
        assert_eq!(git(&root, &["rev-parse", "main"]), base);
        assert_eq!(
            std::fs::read_to_string(root.join("cache")).unwrap(),
            "user data, not disposable"
        );
    }

    #[test]
    fn recovery_distinguishes_unapplied_partial_and_confirmed_later_changes() {
        let (_temp, root, base, result) = fixture();
        let service = GitService::new();
        let paths = std::slice::from_ref(&root);
        assert_eq!(
            service
                .inspect_exact_publication(&root, "refs/heads/main", &base, &result, false, paths)
                .unwrap(),
            ExactPublicationState::NotApplied
        );
        service
            .promote_exact(
                &root,
                "refs/heads/main",
                &base,
                &result,
                paths,
                "crash-before-db",
            )
            .unwrap();
        // Lost DB receipt, exact R and clean checkout still prove application.
        assert_eq!(
            service
                .inspect_exact_publication(&root, "refs/heads/main", &base, &result, false, paths)
                .unwrap(),
            ExactPublicationState::Applied
        );
        std::fs::write(root.join("later"), "later Wiki or source update").unwrap();
        git(&root, &["add", "later"]);
        git(&root, &["commit", "-m", "later update"]);
        let later = git(&root, &["rev-parse", "HEAD"]);
        assert!(
            service
                .inspect_exact_publication(&root, "refs/heads/main", &base, &result, false, paths)
                .is_err()
        );
        std::fs::write(
            root.join("source"),
            "new user work after confirmed publication",
        )
        .unwrap();
        assert_eq!(
            service
                .inspect_exact_publication(&root, "refs/heads/main", &base, &result, true, paths)
                .unwrap(),
            ExactPublicationState::Applied
        );
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), later);
        assert_eq!(
            std::fs::read_to_string(root.join("source")).unwrap(),
            "new user work after confirmed publication"
        );

        let (_temp, root, base, result) = fixture();
        let paths = std::slice::from_ref(&root);
        service
            .promote_exact_inner(
                &root,
                "refs/heads/main",
                &base,
                &result,
                paths,
                "interrupted-checkout",
                || Err(GitServiceError::IoError(std::io::Error::other("crash"))),
            )
            .unwrap_err();
        assert!(
            service
                .inspect_exact_publication(&root, "refs/heads/main", &base, &result, false, paths)
                .is_err()
        );
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), base);
        assert_eq!(
            std::fs::read_to_string(root.join("source")).unwrap(),
            "verified"
        );
    }

    #[test]
    fn noop_recovery_is_applied_without_an_empty_commit() {
        let (_temp, root, base, _) = fixture();
        assert_eq!(
            GitService::new()
                .inspect_exact_publication(
                    &root,
                    "refs/heads/main",
                    &base,
                    &base,
                    false,
                    std::slice::from_ref(&root)
                )
                .unwrap(),
            ExactPublicationState::Applied
        );
        assert_eq!(git(&root, &["rev-list", "--count", "main"]), "1");
    }
}
