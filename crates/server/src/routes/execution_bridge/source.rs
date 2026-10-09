use std::{collections::HashSet, io::Read, path::Path, process::Command};

use anyhow::{Context, ensure};
use axum::{Json, extract::State};
use base64::{Engine, engine::general_purpose::STANDARD};
use db::models::{session::Session, workspace::Workspace};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use serde_json::json;
use services::services::container::ContainerService;
use sha2::{Digest, Sha256};
use utils::response::ApiResponse;
use uuid::Uuid;

use super::error;
use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceFile {
    pub path: String,
    pub sha256: String,
    pub content: String,
}

pub(super) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|p| {
            let stem = p.split('.').next().unwrap_or("").to_ascii_uppercase();
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with([' ', '.'])
                && !p.chars().any(|c| c.is_control() || "<>\"|?*".contains(c))
                && !matches!(
                    stem.as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "CONIN$"
                        | "CONOUT$"
                        | "COM¹"
                        | "COM²"
                        | "COM³"
                        | "LPT¹"
                        | "LPT²"
                        | "LPT³"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
                && !matches!(p.to_ascii_lowercase().as_str(), ".git" | ".lvk-shared")
        })
}
pub(super) fn validate_files(files: &[SourceFile]) -> anyhow::Result<()> {
    ensure!(files.len() <= 10000, "Too many files");
    let mut paths = HashSet::new();
    let mut total = 0;
    for f in files {
        ensure!(
            safe_path(&f.path) && paths.insert(f.path.to_uppercase()),
            "Unsafe or case-colliding path"
        );
        let data = STANDARD.decode(&f.content)?;
        total += data.len();
        ensure!(
            total <= 16 * 1024 * 1024,
            "Snapshot/artifacts exceed 16 MiB"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&data)) == f.sha256,
            "Content digest mismatch"
        );
    }
    for path in &paths {
        let parts: Vec<_> = path.split('/').collect();
        ensure!(
            !(1..parts.len()).any(|i| paths.contains(&parts[..i].join("/"))),
            "File/directory collision on Windows"
        );
    }
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    ensure!(output.status.success(), "Git snapshot inspection failed");
    Ok(output.stdout)
}
fn collect(root: &Path) -> anyhow::Result<Vec<SourceFile>> {
    let list = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut paths = list
        .split(|c| *c == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8(p.to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    paths.dedup();
    ensure!(paths.len() <= 10000, "Too many source files");
    let mut files = vec![];
    let mut total = 0;
    for path in paths {
        ensure!(safe_path(&path), "Unsupported Windows source path: {path}");
        let file = root.join(&path);
        let mut parent = root.to_path_buf();
        let mut deleted = false;
        for part in path.split('/') {
            parent.push(part);
            match parent.symlink_metadata() {
                Ok(meta) => ensure!(
                    !meta.file_type().is_symlink(),
                    "Symlinks are not transferred"
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    deleted = true;
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
        if deleted {
            continue;
        } // Tracked deletion, never a dangling symlink.
        let meta = file.metadata()?;
        ensure!(
            meta.is_file(),
            "Submodules and directories need explicit project packaging"
        );
        let remaining = 16 * 1024 * 1024 - total;
        ensure!(
            meta.len() <= remaining,
            "Source exceeds 16 MiB; exclude build outputs"
        );
        let mut data = Vec::new();
        std::fs::File::open(file)?
            .take(remaining + 1)
            .read_to_end(&mut data)?;
        ensure!(
            data.len() as u64 <= remaining,
            "Source grew during capture; retry after edits settle"
        );
        total += data.len() as u64;
        files.push(SourceFile {
            path,
            sha256: format!("{:x}", Sha256::digest(&data)),
            content: STANDARD.encode(data),
        });
    }
    validate_files(&files)?;
    Ok(files)
}
fn snapshot(root: &Path) -> anyhow::Result<serde_json::Value> {
    let root = root.canonicalize()?;
    let top = String::from_utf8(git(&root, &["rev-parse", "--show-toplevel"])?)?;
    ensure!(
        Path::new(top.trim()).canonicalize()? == root,
        "Choose a repository root within the workspace"
    );
    let head = String::from_utf8(git(&root, &["rev-parse", "HEAD"])?)?
        .trim()
        .to_owned();
    let files = collect(&root)?;
    ensure!(
        collect(&root)? == files
            && String::from_utf8(git(&root, &["rev-parse", "HEAD"])?)?.trim() == head,
        "Source changed during capture; retry after edits settle"
    );
    let mut digest = Sha256::new();
    for f in &files {
        digest.update(f.path.as_bytes());
        digest.update([0]);
        digest.update(f.sha256.as_bytes());
        digest.update(b"\n");
    }
    Ok(json!({"head":head,"digest":format!("{:x}",digest.finalize()),"files":files}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Capture {
    session_id: Uuid,
    working_dir: String,
}
pub(super) async fn capture(
    State(d): State<DeploymentImpl>,
    Json(r): Json<Capture>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    if r.working_dir != "." && !safe_path(&r.working_dir) {
        return Err(error("Invalid source directory"));
    }
    let session = Session::find_by_id(&d.db().pool, r.session_id)
        .await?
        .ok_or_else(|| error("Session missing"))?;
    let workspace = Workspace::find_by_id(&d.db().pool, session.workspace_id)
        .await?
        .ok_or_else(|| error("Workspace missing"))?;
    let root = std::path::PathBuf::from(
        d.container()
            .ensure_container_exists(&workspace)
            .await
            .map_err(error)?,
    )
    .canonicalize()
    .map_err(error)?;
    let path = root.join(r.working_dir).canonicalize().map_err(error)?;
    if !path.starts_with(&root) {
        return Err(error("Source escapes workspace"));
    }
    let manifest = tokio::task::spawn_blocking(move || snapshot(&path))
        .await
        .map_err(error)?
        .map_err(error)?;
    let id = Uuid::new_v4();
    let digest = manifest["digest"]
        .as_str()
        .context("Missing digest")
        .map_err(error)?;
    sqlx::query("INSERT INTO bridge_sources(id,workspace_id,digest,manifest) VALUES(?,?,?,?)")
        .bind(id)
        .bind(workspace.id)
        .bind(digest)
        .bind(sqlx::types::Json(&manifest))
        .execute(&d.db().pool)
        .await?;
    Ok(Json(ApiResponse::success(
        json!({"id":id,"workspace_id":workspace.id,"head":manifest["head"],"digest":digest,"file_count":manifest["files"].as_array().map(Vec::len)}),
    )))
}

pub(super) async fn inspect(
    State(d): State<DeploymentImpl>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<ApiResponse<serde_json::Value>>, ApiError> {
    let (workspace, manifest): (Uuid, sqlx::types::Json<serde_json::Value>) =
        sqlx::query_as("SELECT workspace_id,manifest FROM bridge_sources WHERE id=?")
            .bind(id)
            .fetch_one(&d.db().pool)
            .await?;
    let mut manifest = manifest.0;
    if let Some(files) = manifest["files"].as_array_mut() {
        for file in files {
            if let Some(file) = file.as_object_mut() {
                file.remove("content");
            }
        }
    }
    Ok(Json(ApiResponse::success(
        json!({"id": id, "workspace_id": workspace, "manifest": manifest}),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_paths_and_collisions() {
        for path in [
            "../x",
            "/x",
            "C:x",
            "a\\b",
            "NUL.txt",
            "a/COM1",
            "a.",
            ".git/config",
            "a:b",
            "x\n",
        ] {
            assert!(!safe_path(path), "{path}");
        }
        assert!(safe_path("src/hello world.py"));
        let f = SourceFile {
            path: "a".into(),
            sha256: format!("{:x}", Sha256::digest(b"")),
            content: String::new(),
        };
        let mut other = f.clone();
        other.path = "A".into();
        assert!(validate_files(&[f, other]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_dangling_symlinks_instead_of_silently_omitting_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init"]).unwrap();
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        )
        .unwrap();
        std::os::unix::fs::symlink("missing", root.join("broken")).unwrap();
        assert!(snapshot(root).is_err());
    }
    #[test]
    fn captures_uncommitted_untracked_and_deletions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init"]).unwrap();
        std::fs::write(root.join("tracked.txt"), "old").unwrap();
        std::fs::write(root.join("deleted.txt"), "old").unwrap();
        git(root, &["add", "."]).unwrap();
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        )
        .unwrap();
        std::fs::write(root.join("tracked.txt"), "new").unwrap();
        std::fs::write(root.join("new.txt"), "untracked").unwrap();
        std::fs::remove_file(root.join("deleted.txt")).unwrap();
        let s = snapshot(root).unwrap();
        let files = s["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1]["content"], STANDARD.encode(b"new"));
    }
}
