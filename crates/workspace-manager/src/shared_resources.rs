//! Fixed, repository-scoped local resources. No tool-specific cache policy.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use git::GitService;
use uuid::Uuid;

static PROVISION_LOCK: Mutex<()> = Mutex::new(());
const KINDS: [&str; 2] = ["persistent", "cache"];

pub fn ensure(repo_root: &Path, repo_name: &str, repo_id: Uuid) -> io::Result<()> {
    ensure_at(
        repo_root,
        &utils::path::shared_resources_dir(repo_name, repo_id),
    )
}

fn real_directory(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "Shared directory conflicts with an existing path: {}",
                    path.display()
                )))
            }
        }
        Err(error) => Err(error),
    }
}

fn validate_link(link: &Path, target: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
        Ok(meta) if meta.file_type().is_symlink() && fs::canonicalize(link)? == target => Ok(true),
        Ok(_) => Err(io::Error::other(format!(
            "EVK shared link conflicts with an existing path; move it manually before retrying: {}",
            link.display()
        ))),
    }
}

fn ensure_at(repo_root: &Path, shared: &Path) -> io::Result<()> {
    let _guard = PROVISION_LOCK
        .lock()
        .map_err(|_| io::Error::other("shared resource lock poisoned"))?;
    let repo_root = fs::canonicalize(repo_root)?;
    let common = GitService::new()
        .get_common_dir(&repo_root)
        .map_err(io::Error::other)?;
    let tracked = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .args(["ls-files", "-z", "--", ".evk-shared"])
        .output()?;
    if !tracked.status.success() || !tracked.stdout.is_empty() {
        return Err(io::Error::other(
            "Cannot provision EVK shared directories: .evk-shared is tracked or Git status could not be checked",
        ));
    }
    let parent = shared
        .parent()
        .ok_or_else(|| io::Error::other("missing shared parent"))?;
    fs::create_dir_all(
        parent
            .parent()
            .ok_or_else(|| io::Error::other("missing shared base"))?,
    )?;
    real_directory(parent)?;
    real_directory(shared)?;
    for kind in KINDS {
        real_directory(&shared.join(kind))?;
    }
    let shared = fs::canonicalize(shared)?;
    let mount = repo_root.join(".evk-shared");
    real_directory(&mount)?;
    // Check all targets before creating links; never replace user paths.
    for kind in KINDS {
        validate_link(&mount.join(kind), &shared.join(kind))?;
    }

    let info = common.join("info");
    fs::create_dir_all(&info)?;
    let exclude = info.join("exclude");
    if fs::symlink_metadata(&exclude).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(io::Error::other(
            "Cannot update Git local excludes through a symlink",
        ));
    }
    let previous = match fs::read_to_string(&exclude) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if !previous.lines().any(|line| line.trim() == "/.evk-shared") {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(exclude)?
            .write_all(b"\n# EVK local shared resources\n/.evk-shared\n")?;
    }
    for kind in KINDS {
        let link = mount.join(kind);
        let target = shared.join(kind);
        if validate_link(&link, &target)? {
            continue;
        }
        let relative = pathdiff::diff_paths(&target, &mount).unwrap_or_else(|| target.clone());
        if let Err(error) = create_directory_link(&relative, &link) {
            // Another EVK process may have provisioned the same link meanwhile.
            if error.kind() != io::ErrorKind::AlreadyExists || !validate_link(&link, &target)? {
                return Err(error);
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link).map_err(|error| io::Error::new(error.kind(), format!("Cannot create EVK directory link; enable Windows Developer Mode or run with symlink privileges: {error}")))
}

/// Only return already provisioned paths for this repository, not the shared
/// parent containing other repositories.
pub fn writable_roots(repo_name: &str, repo_id: Uuid) -> Vec<PathBuf> {
    let root = utils::path::shared_resources_dir(repo_name, repo_id);
    KINDS
        .into_iter()
        .filter_map(|kind| fs::canonicalize(root.join(kind)).ok())
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn worktrees_share_data_exclude_links_and_keep_data_after_deletion() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let tree = tmp.path().join("worktree");
        git(
            &repo,
            &["worktree", "add", "-b", "test", tree.to_str().unwrap()],
        );
        let storage = utils::path::shared_resources_dir("repo", Uuid::nil());
        let shared = tmp.path().join("shared").join(storage.file_name().unwrap());
        ensure_at(&repo, &shared).unwrap();
        ensure_at(&tree, &shared).unwrap();
        ensure_at(&tree, &shared).unwrap();
        fs::write(repo.join(".evk-shared/persistent/data"), "retained").unwrap();
        assert_eq!(
            fs::read_to_string(tree.join(".evk-shared/persistent/data")).unwrap(),
            "retained"
        );
        assert!(git(&tree, &["status", "--porcelain"]).is_empty());
        assert!(
            !fs::read_link(tree.join(".evk-shared/cache"))
                .unwrap()
                .is_absolute()
        );
        let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert_eq!(exclude.matches("/.evk-shared").count(), 1);
        assert!(!repo.join(".evk").exists());
        assert_eq!(
            fs::canonicalize(tree.join(".evk-shared/cache")).unwrap(),
            shared.join("cache")
        );
        git(
            &repo,
            &["worktree", "remove", "--force", tree.to_str().unwrap()],
        );
        assert_eq!(
            fs::read_to_string(shared.join("persistent/data")).unwrap(),
            "retained"
        );
    }

    #[test]
    fn refuses_existing_files_and_links_to_other_repositories() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init"]);
        fs::create_dir(repo.join(".evk-shared")).unwrap();
        fs::write(repo.join(".evk-shared/cache"), "user data").unwrap();
        let shared = tmp.path().join("shared").join("first");
        assert!(ensure_at(&repo, &shared).is_err());
        assert_eq!(
            fs::read_to_string(repo.join(".evk-shared/cache")).unwrap(),
            "user data"
        );
        assert!(!repo.join(".evk-shared/persistent").exists());
        fs::remove_file(repo.join(".evk-shared/cache")).unwrap();
        ensure_at(&repo, &shared).unwrap();
        assert!(ensure_at(&repo, &tmp.path().join("shared/other")).is_err());
        assert_eq!(
            fs::canonicalize(repo.join(".evk-shared/cache")).unwrap(),
            shared.join("cache")
        );
    }

    #[test]
    fn refuses_tracked_mount_and_symlinked_storage_without_overwriting() {
        let tmp = tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init"]);
        fs::write(repo.join(".evk-shared"), "tracked config").unwrap();
        git(&repo, &["add", ".evk-shared"]);
        let shared = tmp.path().join("shared/repo-id");
        assert!(ensure_at(&repo, &shared).is_err());
        assert!(!shared.exists());
        assert_eq!(
            fs::read_to_string(repo.join(".evk-shared")).unwrap(),
            "tracked config"
        );
        git(&repo, &["rm", "--cached", ".evk-shared"]);
        fs::remove_file(repo.join(".evk-shared")).unwrap();
        fs::create_dir_all(&shared).unwrap();
        let external = tmp.path().join("external");
        fs::create_dir(&external).unwrap();
        create_directory_link(&external, &shared.join("persistent")).unwrap();
        assert!(ensure_at(&repo, &shared).is_err());
        assert!(!repo.join(".evk-shared").exists());
        assert!(fs::read_dir(external).unwrap().next().is_none());
    }
}
