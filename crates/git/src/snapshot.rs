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
        let repo = self.open_repo(root)?;
        let tree = repo.find_commit(Oid::from_str(source)?)?.tree_id();
        Ok(SnapshotReader { repo, tree })
    }
}

impl SnapshotReader {
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
