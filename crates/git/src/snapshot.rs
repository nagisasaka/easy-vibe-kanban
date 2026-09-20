//! Read-only, pinned Git snapshot access. Callers select paths before blob reads.
use std::path::Path;

use git2::{ObjectType, Oid, Repository, TreeWalkMode, TreeWalkResult};

use crate::{GitService, GitServiceError};

#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub path: String,
    pub oid: String,
    pub mode: i32,
}

pub struct SnapshotReader {
    repo: Repository,
    tree: Oid,
}

impl GitService {
    pub fn snapshot(&self, root: &Path, source: &str) -> Result<SnapshotReader, GitServiceError> {
        if source.len() != 40 || !source.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(GitServiceError::InvalidRepository(
                "Snapshot requires a full commit OID, not a branch or abbreviated identity".into(),
            ));
        }
        let repo = self.open_repo(root)?;
        let tree = repo.find_commit(Oid::from_str(source)?)?.tree_id();
        Ok(SnapshotReader { repo, tree })
    }
}

impl SnapshotReader {
    pub fn entry(&self, path: &str) -> Result<SnapshotEntry, GitServiceError> {
        if path.is_empty()
            || path.contains(['\\', ':', '\0'])
            || path.starts_with('/')
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(GitServiceError::InvalidRepository(
                "Expected a repository-relative snapshot path".into(),
            ));
        }
        let tree = self.repo.find_tree(self.tree)?;
        let entry = tree.get_path(Path::new(path))?;
        Ok(SnapshotEntry {
            path: path.into(),
            oid: entry.id().to_string(),
            mode: entry.filemode(),
        })
    }

    /// Only tree objects are read. Symlinks and gitlinks are returned, never followed.
    pub fn entries(&self) -> Result<Vec<SnapshotEntry>, GitServiceError> {
        let tree = self.repo.find_tree(self.tree)?;
        let mut entries = Vec::new();
        let mut invalid_name = false;
        tree.walk(TreeWalkMode::PreOrder, |prefix, entry| {
            let Some(name) = entry.name() else {
                invalid_name = true;
                return TreeWalkResult::Abort;
            };
            if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\']) {
                invalid_name = true;
                return TreeWalkResult::Abort;
            }
            if entry.kind() != Some(ObjectType::Tree) {
                entries.push(SnapshotEntry {
                    path: format!("{prefix}{name}"),
                    oid: entry.id().to_string(),
                    mode: entry.filemode(),
                });
            }
            TreeWalkResult::Ok
        })?;
        if invalid_name {
            return Err(GitServiceError::InvalidRepository(
                "Snapshot contains a non-UTF-8 path".into(),
            ));
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }

    /// None means the object exceeds the explicit limit, not an absent/unreadable file.
    pub fn read_blob(
        &self,
        entry: &SnapshotEntry,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, GitServiceError> {
        if !matches!(entry.mode, 0o100644 | 0o100755) {
            return Err(GitServiceError::InvalidRepository(
                "Snapshot entry is not a regular file".into(),
            ));
        }
        let oid = Oid::from_str(&entry.oid)?;
        let (size, kind) = self.repo.odb()?.read_header(oid)?;
        if kind != ObjectType::Blob {
            return Err(GitServiceError::InvalidRepository(
                "Snapshot entry is not a blob".into(),
            ));
        }
        if size > limit {
            return Ok(None);
        }
        Ok(Some(self.repo.find_blob(oid)?.content().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_reads_need_no_checkout_and_never_follow_links_or_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        let signature = git2::Signature::now("Test", "test@example.com").unwrap();
        let empty = repo.treebuilder(None).unwrap().write().unwrap();
        let base = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "base",
                &repo.find_tree(empty).unwrap(),
                &[],
            )
            .unwrap();
        let mut builder = repo.treebuilder(None).unwrap();
        let bytes = "# 日本語\nNo checkout required\n".as_bytes();
        builder
            .insert("README.md", repo.blob(bytes).unwrap(), 0o100644)
            .unwrap();
        builder
            .insert("link", repo.blob(b"/outside/private").unwrap(), 0o120000)
            .unwrap();
        builder.insert("submodule", base, 0o160000).unwrap();
        let tree = builder.write().unwrap();
        let source = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "source",
                &repo.find_tree(tree).unwrap(),
                &[&repo.find_commit(base).unwrap()],
            )
            .unwrap()
            .to_string();
        let git = GitService::new();
        for invalid in ["HEAD", "main", "--help", &source[..12]] {
            assert!(git.snapshot(temp.path(), invalid).is_err());
        }
        let snapshot = git.snapshot(temp.path(), &source).unwrap();
        assert!(!temp.path().join("README.md").exists());
        assert_eq!(snapshot.entries().unwrap().len(), 3);
        let file = snapshot.entry("README.md").unwrap();
        assert_eq!(snapshot.read_blob(&file, 1024).unwrap().unwrap(), bytes);
        assert!(snapshot.read_blob(&file, 1).unwrap().is_none());
        for invalid in [
            "../README.md",
            "/README.md",
            "a/../../README.md",
            ".git/config",
            "README.md/child",
            "link/child",
            "submodule/child",
        ] {
            assert!(snapshot.entry(invalid).is_err());
        }
        for path in ["link", "submodule"] {
            assert!(
                snapshot
                    .read_blob(&snapshot.entry(path).unwrap(), 1024)
                    .is_err()
            );
        }
        assert!(snapshot.entry("missing.md").is_err());
        assert_eq!(
            repo.head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string(),
            source
        );
    }
}
