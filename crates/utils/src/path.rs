use std::path::{Path, PathBuf};

use crate::assets::asset_dir;

/// Directory name for storing attachments in worktrees
pub const VIBE_ATTACHMENTS_DIR: &str = ".vibe-attachments";

/// Directories that should always be skipped regardless of gitignore.
/// .git is not in .gitignore but should never be watched.
pub const ALWAYS_SKIP_DIRS: &[&str] = &[".git", "node_modules", ".evk-shared"];

/// Convert absolute paths to relative paths based on worktree path
/// This is a robust implementation that handles symlinks and edge cases
pub fn make_path_relative(path: &str, worktree_path: &str) -> String {
    tracing::trace!("Making path relative: {} -> {}", path, worktree_path);

    let path_obj = normalize_macos_private_alias(Path::new(&path));
    let worktree_path_obj = normalize_macos_private_alias(Path::new(worktree_path));

    if let Ok(relative_path) = path_obj.strip_prefix(&worktree_path_obj) {
        let result = relative_path.to_string_lossy().to_string();
        tracing::trace!("Successfully made relative: '{}' -> '{}'", path, result);
        if result.is_empty() {
            return ".".to_string();
        }
        return result;
    }

    // If path is already relative, return as is. This check must happen after
    // strip_prefix because Windows treats POSIX-style paths like /tmp/foo as
    // relative, while agent logs can still report paths in that format.
    if path_obj.is_relative() {
        return path.to_string();
    }

    if !path_obj.exists() || !worktree_path_obj.exists() {
        return path.to_string();
    }

    // canonicalize may fail if paths don't exist
    let canonical_path = std::fs::canonicalize(&path_obj);
    let canonical_worktree = std::fs::canonicalize(&worktree_path_obj);

    match (canonical_path, canonical_worktree) {
        (Ok(canon_path), Ok(canon_worktree)) => {
            tracing::debug!(
                "Trying canonical path resolution: '{}' -> '{}', '{}' -> '{}'",
                path,
                canon_path.display(),
                worktree_path,
                canon_worktree.display()
            );

            match canon_path.strip_prefix(&canon_worktree) {
                Ok(relative_path) => {
                    let result = relative_path.to_string_lossy().to_string();
                    tracing::debug!(
                        "Successfully made relative with canonical paths: '{}' -> '{}'",
                        path,
                        result
                    );
                    if result.is_empty() {
                        return ".".to_string();
                    }
                    result
                }
                Err(e) => {
                    tracing::debug!(
                        "Failed to make canonical path relative: '{}' relative to '{}', error: {}, returning original",
                        canon_path.display(),
                        canon_worktree.display(),
                        e
                    );
                    path.to_string()
                }
            }
        }
        _ => {
            tracing::debug!(
                "Could not canonicalize paths (paths may not exist): '{}', '{}', returning original",
                path,
                worktree_path
            );
            path.to_string()
        }
    }
}

/// Normalize macOS prefix /private/var/ and /private/tmp/ to their public aliases without resolving paths.
/// This allows prefix normalization to work when the full paths don't exist.
pub fn normalize_macos_private_alias<P: AsRef<Path>>(p: P) -> PathBuf {
    let p = p.as_ref();
    if cfg!(target_os = "macos")
        && let Some(s) = p.to_str()
    {
        if s == "/private/var" {
            return PathBuf::from("/var");
        }
        if let Some(rest) = s.strip_prefix("/private/var/") {
            return PathBuf::from(format!("/var/{rest}"));
        }
        if s == "/private/tmp" {
            return PathBuf::from("/tmp");
        }
        if let Some(rest) = s.strip_prefix("/private/tmp/") {
            return PathBuf::from(format!("/tmp/{rest}"));
        }
    }
    p.to_path_buf()
}

pub fn get_vibe_kanban_temp_dir() -> std::path::PathBuf {
    let dir_name = if cfg!(debug_assertions) {
        "vibe-kanban-dev"
    } else {
        "vibe-kanban"
    };

    if cfg!(target_os = "macos") {
        // macOS already uses /var/folders/... which is persistent storage
        std::env::temp_dir().join(dir_name)
    } else if cfg!(target_os = "linux") {
        // Linux: use /var/tmp instead of /tmp to avoid RAM usage
        std::path::PathBuf::from("/var/tmp").join(dir_name)
    } else {
        // Windows and other platforms: use temp dir with vibe-kanban subdirectory
        std::env::temp_dir().join(dir_name)
    }
}

/// User-editable card pipeline definitions seeded by the application.
pub fn pipelines_dir() -> PathBuf {
    asset_dir().join("pipelines")
}

/// Shared local resources live outside the worktree cleanup tree, even when
/// the workspace directory is overridden. Use the immutable registered repo
/// name, not its editable display name. The full ID keeps names unambiguous.
pub fn shared_resources_dir(repo_name: &str, repo_id: uuid::Uuid) -> PathBuf {
    // Keep this a single portable path component and leave room for the UUID
    // within common filesystem component limits (48 UTF-8 chars <= 192 bytes).
    let name: String = repo_name
        .chars()
        .take(48)
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches(['.', '-', '_']);
    let name = if name.is_empty() { "repository" } else { name };
    get_vibe_kanban_temp_dir()
        .join("shared")
        .join(format!("{name}-{repo_id}"))
}

/// Application-managed Codex skills bundled with LLM Wiki.
pub fn llm_wiki_skills_dir() -> PathBuf {
    asset_dir().join("skills").join("llm-wiki")
}

/// Expand leading ~ to user's home directory.
pub fn expand_tilde(path_str: &str) -> std::path::PathBuf {
    shellexpand::tilde(path_str).as_ref().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_resources_name_is_readable_and_uses_the_full_repo_id() {
        let id = uuid::Uuid::parse_str("12b6186f-0781-4e73-a844-178dd9646aa1").unwrap();
        let path = shared_resources_dir("easy-vibe-kanban", id);
        assert_eq!(
            path,
            get_vibe_kanban_temp_dir()
                .join("shared/easy-vibe-kanban-12b6186f-0781-4e73-a844-178dd9646aa1")
        );
        assert_ne!(
            path,
            shared_resources_dir("easy-vibe-kanban", uuid::Uuid::nil())
        );
    }

    #[test]
    fn shared_resources_name_cannot_escape_storage_or_exceed_component_limits() {
        let id = uuid::Uuid::nil();
        let parent = get_vibe_kanban_temp_dir().join("shared");
        for name in [
            "../../another/repo",
            "C:\\repo\\name",
            "repo name:*?",
            "...",
            "",
            &"日".repeat(300),
        ] {
            let path = shared_resources_dir(name, id);
            assert_eq!(path.parent(), Some(parent.as_path()));
            assert!(path.file_name().unwrap().len() <= 255);
            assert!(
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .ends_with(&id.to_string())
            );
        }
        assert_eq!(
            shared_resources_dir("", id).file_name().unwrap(),
            format!("repository-{id}").as_str()
        );
    }

    #[test]
    fn test_make_path_relative() {
        // Test with relative path (should remain unchanged)
        assert_eq!(
            make_path_relative("src/main.rs", "/tmp/test-worktree"),
            "src/main.rs"
        );

        // Test with absolute path (should become relative if possible)
        let test_worktree = "/tmp/test-worktree";
        let absolute_path = format!("{test_worktree}/src/main.rs");
        let result = make_path_relative(&absolute_path, test_worktree);
        assert_eq!(result, "src/main.rs");

        // Test with path outside worktree (should return original)
        assert_eq!(
            make_path_relative("/other/path/file.js", "/tmp/test-worktree"),
            "/other/path/file.js"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_make_path_relative_macos_private_alias() {
        // Simulate a worktree under /var with a path reported under /private/var
        let worktree = "/var/folders/zz/abc123/T/vibe-kanban-dev/worktrees/vk-test";
        let path_under_private = format!(
            "/private/var{}/hello-world.txt",
            worktree.strip_prefix("/var").unwrap()
        );
        assert_eq!(
            make_path_relative(&path_under_private, worktree),
            "hello-world.txt"
        );

        // Also handle the inverse: worktree under /private and path under /var
        let worktree_private = format!("/private{worktree}");
        let path_under_var = format!("{worktree}/hello-world.txt");
        assert_eq!(
            make_path_relative(&path_under_var, &worktree_private),
            "hello-world.txt"
        );
    }
}
