//! Disposable OpenWiki instruction updates in an existing maintenance worktree.
//! The shared-folder journal protects user files; no upstream code or metadata
//! is modified. Call preparation before launch and restoration only after exit.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, bail, ensure};
use utils::repository_memory::{
    RepositoryMemoryStore, WikiSetupCheckpoint, WikiSetupEntry, WikiSetupFile, WikiSetupPhase,
    reject_symlinks,
};
use uuid::Uuid;

const INSTRUCTION_PATHS: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
const MAX_INSTRUCTION_BYTES: u64 = 32 * 1024;
const MAX_PAGE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_WIKI_ENTRIES: usize = 20_000;

/// Setup also writes a CI file. Checking parents before the model starts is
/// essential: rejecting a symlink at publication cannot undo an outside write.
pub fn preflight(root: &Path) -> anyhow::Result<()> {
    reject_symlinks(root)?;
    ensure!(root.is_dir(), "Maintenance repository is missing");
    for path in INSTRUCTION_PATHS {
        let entry = capture_instruction(root, path)?;
        if let WikiSetupFile::Regular { content, .. } = &entry.prepared {
            super::without_managed_block(content).with_context(|| {
                format!("Invalid existing OpenWiki markers in {path}; no model was started")
            })?;
        }
    }
    for path in [
        ".github/workflows/openwiki-update.yml",
        ".codex/config.toml",
        ".agents/skills/openwiki",
        "openwiki/INSTRUCTIONS.md",
    ] {
        reject_symlinks(&root.join(path))
            .with_context(|| format!("Unsafe OpenWiki setup path: {path}"))?;
        if path != ".agents/skills/openwiki" {
            match fs::symlink_metadata(root.join(path)) {
                Ok(metadata) => {
                    ensure!(metadata.is_file(), "Special OpenWiki setup file: {path}");
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        ensure!(
                            metadata.nlink() == 1,
                            "Hard-linked OpenWiki setup file: {path}"
                        );
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    wiki_files(root)?;
    Ok(())
}

/// Idempotent before launch, including a crash between two materialisations.
/// Existing internal symlinks become independent files, not aliases of one inode.
pub fn prepare(
    store: &RepositoryMemoryStore,
    workspace_id: Uuid,
    root: &Path,
    source_commit: &str,
) -> anyhow::Result<()> {
    let mut checkpoint = if let Some(checkpoint) = store.wiki_setup(workspace_id)? {
        validate_identity(&checkpoint, workspace_id, root, source_commit)?;
        checkpoint
    } else {
        preflight(root)?;
        let checkpoint = WikiSetupCheckpoint {
            version: 1,
            workspace_id,
            repository_path: root.into(),
            source_commit: source_commit.into(),
            phase: WikiSetupPhase::Preparing,
            files: INSTRUCTION_PATHS
                .iter()
                .map(|path| capture_instruction(root, path))
                .collect::<anyhow::Result<_>>()?,
        };
        store.save_wiki_setup(&checkpoint)?;
        checkpoint
    };
    ensure!(
        matches!(
            checkpoint.phase,
            WikiSetupPhase::Preparing | WikiSetupPhase::Prepared
        ),
        "OpenWiki setup has already been restored; do not relaunch an old reservation"
    );
    ensure!(
        super::git_text(root, &["rev-parse", "HEAD"])? == source_commit,
        "Maintenance source changed before instruction preparation"
    );
    // Validate every entry before modifying any of them.
    for entry in &checkpoint.files {
        let current = read_entry(&root.join(&entry.path))?;
        if checkpoint.phase == WikiSetupPhase::Prepared {
            validate_generated_change(entry, &current)?;
        } else {
            ensure!(
                current == entry.original || current == entry.prepared,
                "{} changed during OpenWiki preparation; preserved for inspection",
                entry.path
            );
        }
    }
    if checkpoint.phase == WikiSetupPhase::Preparing {
        for entry in &checkpoint.files {
            replace_if_expected(&root.join(&entry.path), &entry.original, &entry.prepared)?;
        }
        checkpoint.phase = WikiSetupPhase::Prepared;
        store.save_wiki_setup(&checkpoint)?;
    }
    Ok(())
}

/// No journal means a legacy run: do not invent an original snapshot. The
/// publication guard still rejects its instruction changes until reviewed.
pub fn restore(
    store: &RepositoryMemoryStore,
    workspace_id: Uuid,
    root: &Path,
    source_commit: &str,
) -> anyhow::Result<()> {
    let Some(mut checkpoint) = store.wiki_setup(workspace_id)? else {
        return Ok(());
    };
    validate_identity(&checkpoint, workspace_id, root, source_commit)?;
    ensure!(
        super::git_text(root, &["rev-parse", "HEAD"])? == source_commit,
        "Maintenance Git history changed; instruction restoration refused"
    );
    if checkpoint.phase == WikiSetupPhase::Restored {
        for entry in &checkpoint.files {
            ensure!(
                read_entry(&root.join(&entry.path))? == entry.original,
                "{} changed after OpenWiki restoration; preserved for inspection",
                entry.path
            );
        }
        return Ok(());
    }
    if checkpoint.phase != WikiSetupPhase::Restoring {
        for entry in &mut checkpoint.files {
            let current = read_entry(&root.join(&entry.path))?;
            validate_generated_change(entry, &current)?;
            entry.restoring_from = Some(current);
        }
        checkpoint.phase = WikiSetupPhase::Restoring;
        // Durability point: a restarted server can distinguish an already
        // restored file from a later edit, without parsing the new edit away.
        store.save_wiki_setup(&checkpoint)?;
    }
    for entry in &checkpoint.files {
        let current = read_entry(&root.join(&entry.path))?;
        ensure!(
            current == entry.original || Some(&current) == entry.restoring_from.as_ref(),
            "{} changed during OpenWiki restoration; preserved for inspection",
            entry.path
        );
    }
    for entry in &checkpoint.files {
        replace_if_expected(
            &root.join(&entry.path),
            entry
                .restoring_from
                .as_ref()
                .context("Missing restoration checkpoint")?,
            &entry.original,
        )?;
    }
    checkpoint.phase = WikiSetupPhase::Restored;
    store.save_wiki_setup(&checkpoint)?;
    Ok(())
}

/// Begin a *new* writer phase after a completed restoration. The original
/// snapshot is retained; this never reopens an old AgentRun reservation.
pub fn prepare_next_phase(
    store: &RepositoryMemoryStore,
    workspace_id: Uuid,
    root: &Path,
    source_commit: &str,
) -> anyhow::Result<()> {
    restore(store, workspace_id, root, source_commit)?;
    let mut checkpoint = store
        .wiki_setup(workspace_id)?
        .context("Missing original instruction journal")?;
    validate_identity(&checkpoint, workspace_id, root, source_commit)?;
    ensure!(
        checkpoint.phase == WikiSetupPhase::Restored,
        "Previous writer phase is not restored"
    );
    checkpoint.phase = WikiSetupPhase::Preparing;
    for entry in &mut checkpoint.files {
        entry.restoring_from = None;
    }
    store.save_wiki_setup(&checkpoint)?;
    prepare(store, workspace_id, root, source_commit)
}

fn validate_identity(
    checkpoint: &WikiSetupCheckpoint,
    workspace_id: Uuid,
    root: &Path,
    source_commit: &str,
) -> anyhow::Result<()> {
    ensure!(
        checkpoint.version == 1
            && checkpoint.workspace_id == workspace_id
            && checkpoint.repository_path == root
            && checkpoint.source_commit == source_commit,
        "OpenWiki setup checkpoint identity mismatch"
    );
    ensure!(
        checkpoint.files.len() == 2
            && checkpoint
                .files
                .iter()
                .zip(INSTRUCTION_PATHS)
                .all(|(entry, path)| entry.path == path),
        "Invalid OpenWiki setup checkpoint paths"
    );
    reject_symlinks(root)?;
    Ok(())
}

fn capture_instruction(root: &Path, path: &str) -> anyhow::Result<WikiSetupEntry> {
    let absolute = root.join(path);
    let original = read_entry(&absolute)?;
    let prepared = match &original {
        WikiSetupFile::Symlink { .. } => {
            let target = fs::canonicalize(&absolute)
                .with_context(|| format!("Dangling or cyclic instruction link: {path}"))?;
            ensure!(
                target.starts_with(fs::canonicalize(root)?),
                "Instruction link {path} escapes its maintenance repository; no model was started"
            );
            reject_symlinks(&target)?;
            read_regular(&target, MAX_INSTRUCTION_BYTES)?
        }
        file => file.clone(),
    };
    let prepared = match prepared {
        // Writable temporary copies also support repositories with read-only
        // instructions. The exact original mode is restored afterwards.
        WikiSetupFile::Regular { content, mode } => WikiSetupFile::Regular {
            content,
            mode: mode | 0o200,
        },
        file => file,
    };
    Ok(WikiSetupEntry {
        path: path.into(),
        original,
        prepared,
        restoring_from: None,
    })
}

fn read_entry(path: &Path) -> anyhow::Result<WikiSetupFile> {
    reject_symlinks(path.parent().context("Missing instruction parent")?)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WikiSetupFile::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.is_symlink() {
        return Ok(WikiSetupFile::Symlink {
            target: fs::read_link(path)?,
        });
    }
    read_regular(path, MAX_INSTRUCTION_BYTES)
}

fn read_regular(path: &Path, limit: u64) -> anyhow::Result<WikiSetupFile> {
    reject_symlinks(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "OpenWiki setup requires regular files: {}",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.nlink() == 1,
            "Hard-linked OpenWiki input is unsafe: {}",
            path.display()
        );
    }
    ensure!(
        metadata.len() <= limit,
        "OpenWiki input exceeds size limit: {}",
        path.display()
    );
    let mut content = String::new();
    file.take(limit + 1).read_to_string(&mut content)?;
    ensure!(
        content.len() as u64 <= limit,
        "OpenWiki input exceeds size limit"
    );
    Ok(WikiSetupFile::Regular {
        content,
        mode: file_mode(&metadata),
    })
}

fn file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        if metadata.permissions().readonly() {
            0o444
        } else {
            0o644
        }
    }
}

fn validate_generated_change(
    entry: &WikiSetupEntry,
    current: &WikiSetupFile,
) -> anyhow::Result<()> {
    if current == &entry.original || current == &entry.prepared {
        return Ok(());
    }
    let before = match &entry.prepared {
        WikiSetupFile::Missing => "",
        WikiSetupFile::Regular { content, .. } => content,
        WikiSetupFile::Symlink { .. } => bail!("Invalid prepared instruction snapshot"),
    };
    let WikiSetupFile::Regular { content, mode } = current else {
        bail!(
            "{} changed type during OpenWiki; restoration refused",
            entry.path
        );
    };
    if let WikiSetupFile::Regular {
        mode: before_mode, ..
    } = &entry.prepared
    {
        ensure!(
            mode == before_mode,
            "{} changed mode during OpenWiki; restoration refused",
            entry.path
        );
    }
    ensure!(
        super::without_managed_block(before)? == super::without_managed_block(content)?,
        "OpenWiki modified user-authored instructions in {}; restoration refused",
        entry.path
    );
    Ok(())
}

/// Replacing the directory entry, never writing through the original link,
/// avoids changing another file or leaving a truncated instruction on restart.
fn replace_if_expected(
    path: &Path,
    expected: &WikiSetupFile,
    next: &WikiSetupFile,
) -> anyhow::Result<()> {
    let current = read_entry(path)?;
    if &current == next {
        return Ok(());
    }
    ensure!(
        &current == expected,
        "Instruction changed before replacement: {}",
        path.display()
    );
    let parent = path.parent().context("Missing instruction parent")?;
    reject_symlinks(parent)?;
    match next {
        WikiSetupFile::Missing => fs::remove_file(path)?,
        WikiSetupFile::Regular { content, mode } => {
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(content.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temporary
                    .as_file()
                    .set_permissions(fs::Permissions::from_mode(*mode))?;
            }
            #[cfg(not(unix))]
            {
                let mut permissions = temporary.as_file().metadata()?.permissions();
                permissions.set_readonly(mode & 0o200 == 0);
                temporary.as_file().set_permissions(permissions)?;
            }
            temporary.as_file().sync_all()?;
            ensure!(
                read_entry(path)? == current,
                "Instruction changed before atomic replacement"
            );
            temporary.persist(path)?;
        }
        WikiSetupFile::Symlink { target } => {
            let temporary = tempfile::tempdir_in(parent)?;
            let link = temporary.path().join("instruction");
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &link)?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(target, &link)?;
            ensure!(
                read_entry(path)? == current,
                "Instruction changed before link restoration"
            );
            fs::rename(link, path)?;
        }
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn wiki_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let wiki = root.join("openwiki");
    reject_symlinks(&wiki)?;
    if !wiki.try_exists()? {
        return Ok(Vec::new());
    }
    let mut directories = vec![wiki];
    let mut pages = Vec::new();
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        reject_symlinks(&directory)?;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            ensure!(
                count <= MAX_WIKI_ENTRIES,
                "OpenWiki tree exceeds inspection limit"
            );
            let kind = entry.file_type()?;
            ensure!(
                !kind.is_symlink(),
                "Symlink in OpenWiki tree: {}",
                entry.path().display()
            );
            if kind.is_dir() {
                directories.push(entry.path());
            } else {
                ensure!(kind.is_file(), "Special file in OpenWiki tree");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    ensure!(entry.metadata()?.nlink() == 1, "Hard link in OpenWiki tree");
                }
                if entry.path().extension().is_some_and(|ext| ext == "md") {
                    pages.push(entry.path());
                }
            }
        }
    }
    Ok(pages)
}

/// OpenWiki publishes page provenance in Markdown frontmatter. Do not inspect
/// or rewrite private Claims versions to make a discarded source look current.
/// Conservative: a whole-file source entry cannot prove a claim only used the
/// unchanged prefix, so reject provenance to any changed setup instruction.
pub fn validate_provenance(
    store: &RepositoryMemoryStore,
    workspace_id: Uuid,
    root: &Path,
    source: &str,
) -> anyhow::Result<()> {
    let checkpoint = store.wiki_setup(workspace_id)?;
    if let Some(checkpoint) = &checkpoint {
        validate_identity(checkpoint, workspace_id, root, source)?;
    }
    for page in wiki_files(root)? {
        if page == root.join("openwiki/INSTRUCTIONS.md") {
            continue;
        }
        let WikiSetupFile::Regular { content, .. } = read_regular(&page, MAX_PAGE_BYTES)? else {
            unreachable!()
        };
        let Some(rest) = content
            .strip_prefix("---\r\n")
            .or_else(|| content.strip_prefix("---\n"))
        else {
            continue;
        };
        let mut yaml = String::new();
        let mut closed = false;
        for line in rest.lines() {
            if line.trim_end() == "---" {
                closed = true;
                break;
            }
            yaml.push_str(line);
            yaml.push('\n');
        }
        ensure!(closed, "Unclosed OpenWiki frontmatter: {}", page.display());
        let metadata: serde_yaml::Value = serde_yaml::from_str(&yaml)
            .with_context(|| format!("Invalid OpenWiki page provenance: {}", page.display()))?;
        let Some(sources) = metadata.get("sources") else {
            continue;
        };
        let sources = sources
            .as_sequence()
            .context("OpenWiki sources must be a sequence")?;
        for item in sources {
            let resource = item
                .get("resource")
                .and_then(serde_yaml::Value::as_str)
                .context("OpenWiki source is missing its resource")?;
            let Some(path) = evidence_path(resource)? else {
                continue;
            };
            if !INSTRUCTION_PATHS.contains(&path.as_str()) && !super::is_setup_byproduct(&path) {
                continue;
            }
            let before = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["show", &format!("{source}:{path}")])
                .output()?;
            let unchanged = before.status.success()
                && matches!(read_entry(&root.join(&path))?, WikiSetupFile::Regular { ref content, .. } if content.as_bytes() == before.stdout)
                && checkpoint.as_ref().is_none_or(|checkpoint| {
                    checkpoint
                        .files
                        .iter()
                        .filter(|entry| entry.path == path)
                        .all(|entry| {
                            entry.original == entry.prepared
                                && entry
                                    .restoring_from
                                    .as_ref()
                                    .is_none_or(|observed| observed == &entry.original)
                        })
                });
            ensure!(
                unchanged,
                "Wiki page {} cites unpublished OpenWiki setup source {path}. Use integrated source/tests/docs as evidence; generated setup instructions are discarded. Wiki files were retained.",
                page.strip_prefix(root).unwrap_or(&page).display()
            );
        }
    }
    Ok(())
}

fn evidence_path(resource: &str) -> anyhow::Result<Option<String>> {
    let Some(tail) = resource.strip_prefix("repo://") else {
        return Ok(None);
    };
    let encoded = tail.split('#').next().unwrap_or_default();
    let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8()?;
    ensure!(
        !decoded.contains(['\\', '?', '\0']),
        "Invalid OpenWiki evidence path"
    );
    let mut parts = Vec::new();
    for component in Path::new(decoded.as_ref()).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().context("Non-UTF8 evidence path")?),
            Component::CurDir => {}
            _ => bail!("Unsafe OpenWiki evidence path"),
        }
    }
    ensure!(!parts.is_empty(), "Empty OpenWiki evidence path");
    Ok(Some(parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENTS: &str = "# User rules\r\nPreserve this text.  \r\n\r\n";
    const CLAUDE: &str = "# Other user rules\n";
    const BLOCK: &str =
        "<!-- OPENWIKI:START -->\nGenerated setup guidance\n<!-- OPENWIKI:END -->\n";

    #[test]
    fn bootstrap_writer_phases_restore_originals_without_moving_head() {
        let fixture = Fixture::new();
        fixture.prepare();
        fixture.write("AGENTS.md", &format!("{AGENTS}{BLOCK}"));
        fixture.write("openwiki/new.md", "generated page");
        restore(&fixture.store, fixture.id, &fixture.root, &fixture.source).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
            AGENTS
        );
        assert!(prepare(&fixture.store, fixture.id, &fixture.root, &fixture.source).is_err());
        prepare_next_phase(&fixture.store, fixture.id, &fixture.root, &fixture.source).unwrap();
        fixture.write("AGENTS.md", &format!("{AGENTS}{BLOCK}"));
        fixture.write("openwiki/new.md", "refined page");
        restore(&fixture.store, fixture.id, &fixture.root, &fixture.source).unwrap();
        assert_eq!(
            fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
            AGENTS
        );
        assert_eq!(
            super::super::git_text(&fixture.root, &["rev-parse", "HEAD"]).unwrap(),
            fixture.source
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("openwiki/new.md")).unwrap(),
            "refined page"
        );
    }

    struct Fixture {
        temp: tempfile::TempDir,
        root: PathBuf,
        store: RepositoryMemoryStore,
        id: Uuid,
        source: String,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("repo");
            let persistent = temp.path().join("persistent");
            fs::create_dir_all(root.join("openwiki")).unwrap();
            fs::create_dir(&persistent).unwrap();
            let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
            super::super::git_text(&root, &["init", "-b", "main"]).unwrap();
            super::super::git_text(&root, &["config", "user.email", "fixture@example.invalid"])
                .unwrap();
            super::super::git_text(&root, &["config", "user.name", "Fixture"]).unwrap();
            let mut fixture = Self {
                temp,
                root,
                store,
                id: Uuid::new_v4(),
                source: String::new(),
            };
            fixture.write("AGENTS.md", AGENTS);
            fixture.write("CLAUDE.md", CLAUDE);
            fixture.write("source.txt", "integrated source\n");
            fixture.write("openwiki/index.md", "initial wiki\n");
            fixture.write(
                "openwiki/INSTRUCTIONS.md",
                "user-authored wiki instructions\n",
            );
            fixture.commit();
            fixture
        }

        fn write(&self, path: &str, content: &str) {
            fs::create_dir_all(self.root.join(path).parent().unwrap()).unwrap();
            fs::write(self.root.join(path), content).unwrap();
        }

        fn commit(&mut self) {
            super::super::git_text(&self.root, &["add", "."]).unwrap();
            super::super::git_text(&self.root, &["commit", "-m", "fixture"]).unwrap();
            self.source = super::super::git_text(&self.root, &["rev-parse", "HEAD"]).unwrap();
        }

        fn prepare(&self) {
            prepare(&self.store, self.id, &self.root, &self.source).unwrap();
        }
        fn restore(&self) {
            restore(&self.store, self.id, &self.root, &self.source).unwrap();
        }

        fn generate(&self) {
            for path in INSTRUCTION_PATHS {
                let text = fs::read_to_string(self.root.join(path)).unwrap_or_default();
                self.write(path, &format!("{}\n\n{BLOCK}", text.trim_end()));
            }
            self.write("openwiki/index.md", "generated wiki\n");
            self.write(
                ".github/workflows/openwiki-update.yml",
                "generated optional workflow\n",
            );
        }

        fn page(&self, resource: &str) {
            self.write("openwiki/page.md", &format!("---\ntype: concept\ntitle: Fixture\nsources:\n  - id: fixture-source\n    resource: {resource}\n---\n# Fixture\n"));
        }

        fn provenance(&self) -> anyhow::Result<()> {
            validate_provenance(&self.store, self.id, &self.root, &self.source)
        }
    }

    #[test]
    fn ordinary_instructions_restore_exact_bytes_and_only_wiki_is_published() {
        let fixture = Fixture::new();
        fixture.prepare();
        fixture.generate();
        fixture.page("repo://source.txt#L1");
        fixture.provenance().unwrap();
        fixture.restore();
        fixture.restore();
        fixture.provenance().unwrap();
        assert_eq!(
            fs::read(fixture.root.join("AGENTS.md")).unwrap(),
            AGENTS.as_bytes()
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("CLAUDE.md")).unwrap(),
            CLAUDE
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("openwiki/index.md")).unwrap(),
            "generated wiki\n"
        );
        let paths = super::super::publication_paths(
            &git::GitService::new(),
            &fixture.root,
            &fixture.source,
        )
        .unwrap();
        assert!(!paths.is_empty());
        assert!(paths.iter().all(|path| path.starts_with("openwiki/")));
        assert!(
            fixture
                .root
                .join(".github/workflows/openwiki-update.yml")
                .is_file()
        );
    }

    #[test]
    fn newly_created_instruction_files_are_removed_not_empty_files() {
        let mut fixture = Fixture::new();
        for path in INSTRUCTION_PATHS {
            fs::remove_file(fixture.root.join(path)).unwrap();
        }
        fixture.commit();
        fixture.prepare();
        fixture.generate();
        fixture.restore();
        for path in INSTRUCTION_PATHS {
            assert!(!fixture.root.join(path).exists());
        }
        assert!(fixture.root.join("openwiki/index.md").exists());
    }

    #[test]
    fn import_and_preexisting_managed_blocks_preserve_user_prefix_and_suffix() {
        let mut fixture = Fixture::new();
        let original = format!("{AGENTS}{BLOCK}\nUser appendix\n");
        fixture.write("AGENTS.md", &original);
        fixture.write("CLAUDE.md", "@AGENTS.md\n");
        fixture.commit();
        fixture.prepare();
        fixture.write(
            "AGENTS.md",
            &original.replace("Generated setup guidance", "Updated setup guidance"),
        );
        fixture.restore();
        assert_eq!(
            fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("CLAUDE.md")).unwrap(),
            "@AGENTS.md\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn internal_alias_becomes_an_independent_file_and_original_link_returns() {
        let mut fixture = Fixture::new();
        fs::remove_file(fixture.root.join("CLAUDE.md")).unwrap();
        std::os::unix::fs::symlink("AGENTS.md", fixture.root.join("CLAUDE.md")).unwrap();
        fixture.commit();
        fixture.prepare();
        assert!(
            fs::symlink_metadata(fixture.root.join("CLAUDE.md"))
                .unwrap()
                .is_file()
        );
        fixture.write("CLAUDE.md", &format!("{}{BLOCK}", AGENTS.trim_end()));
        assert_eq!(
            fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
            AGENTS
        );
        prepare(
            &RepositoryMemoryStore::at_persistent(&fixture.temp.path().join("persistent")).unwrap(),
            fixture.id,
            &fixture.root,
            &fixture.source,
        )
        .unwrap();
        fixture.restore();
        assert_eq!(
            fs::read_link(fixture.root.join("CLAUDE.md")).unwrap(),
            Path::new("AGENTS.md")
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
            AGENTS
        );
        assert!(
            super::super::git_text(&fixture.root, &["diff", "--", "AGENTS.md", "CLAUDE.md"])
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_dangling_and_hard_links_fail_before_any_instruction_write() {
        for kind in ["external", "dangling", "hard"] {
            let fixture = Fixture::new();
            let outside = fixture.temp.path().join("outside.md");
            fs::write(&outside, "outside instructions").unwrap();
            fs::remove_file(fixture.root.join("CLAUDE.md")).unwrap();
            match kind {
                "external" => {
                    std::os::unix::fs::symlink(&outside, fixture.root.join("CLAUDE.md")).unwrap()
                }
                "dangling" => {
                    std::os::unix::fs::symlink("missing.md", fixture.root.join("CLAUDE.md"))
                        .unwrap()
                }
                _ => fs::hard_link(&outside, fixture.root.join("CLAUDE.md")).unwrap(),
            }
            assert!(prepare(&fixture.store, fixture.id, &fixture.root, &fixture.source).is_err());
            assert!(fixture.store.wiki_setup(fixture.id).unwrap().is_none());
            assert_eq!(fs::read_to_string(outside).unwrap(), "outside instructions");
            assert_eq!(
                fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
                AGENTS
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn setup_parent_and_private_wiki_symlinks_are_rejected() {
        let fixture = Fixture::new();
        let outside = fixture.temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join(".github")).unwrap();
        assert!(preflight(&fixture.root).is_err());
        fs::remove_file(fixture.root.join(".github")).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join("openwiki/.claims")).unwrap();
        assert!(preflight(&fixture.root).is_err());
        assert!(fs::read_dir(outside).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn special_and_hard_linked_setup_files_fail_before_launch() {
        let fixture = Fixture::new();
        let workflow = fixture.root.join(".github/workflows/openwiki-update.yml");
        fs::create_dir_all(workflow.parent().unwrap()).unwrap();
        fs::create_dir(&workflow).unwrap();
        assert!(preflight(&fixture.root).is_err());
        fs::remove_dir(&workflow).unwrap();
        let outside = fixture.temp.path().join("outside.yml");
        fs::write(&outside, "untouched\n").unwrap();
        fs::hard_link(&outside, &workflow).unwrap();
        assert!(preflight(&fixture.root).is_err());
        fs::remove_file(&workflow).unwrap();
        fs::hard_link(&outside, fixture.root.join("openwiki/page.md")).unwrap();
        assert!(preflight(&fixture.root).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "untouched\n");
        assert!(fixture.store.wiki_setup(fixture.id).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn readonly_executable_instruction_mode_is_restored() {
        use std::os::unix::fs::PermissionsExt;
        let mut fixture = Fixture::new();
        fs::set_permissions(
            fixture.root.join("AGENTS.md"),
            fs::Permissions::from_mode(0o1555),
        )
        .unwrap();
        fixture.commit();
        fixture.prepare();
        assert_eq!(
            file_mode(&fs::metadata(fixture.root.join("AGENTS.md")).unwrap()),
            0o1755
        );
        fixture.generate();
        fixture.restore();
        assert_eq!(
            file_mode(&fs::metadata(fixture.root.join("AGENTS.md")).unwrap()),
            0o1555
        );
    }

    #[test]
    fn user_appendix_or_changed_file_type_is_not_discarded() {
        let fixture = Fixture::new();
        fixture.prepare();
        fixture.generate();
        let text = fs::read_to_string(fixture.root.join("CLAUDE.md")).unwrap()
            + "\nUser manual addition\n";
        fixture.write("CLAUDE.md", &text);
        assert!(restore(&fixture.store, fixture.id, &fixture.root, &fixture.source).is_err());
        assert_eq!(
            fs::read_to_string(fixture.root.join("CLAUDE.md")).unwrap(),
            text
        );
        // Validation of all files precedes restoration of any file.
        assert!(
            fs::read_to_string(fixture.root.join("AGENTS.md"))
                .unwrap()
                .contains(BLOCK)
        );
        assert_eq!(
            fixture.store.wiki_setup(fixture.id).unwrap().unwrap().phase,
            WikiSetupPhase::Prepared
        );
        fs::remove_file(fixture.root.join("CLAUDE.md")).unwrap();
        fs::create_dir(fixture.root.join("CLAUDE.md")).unwrap();
        assert!(restore(&fixture.store, fixture.id, &fixture.root, &fixture.source).is_err());
    }

    #[test]
    fn restoration_resumes_after_first_file_and_rejects_later_edits() {
        for edit in [false, true] {
            let fixture = Fixture::new();
            fixture.prepare();
            fixture.generate();
            let mut checkpoint = fixture.store.wiki_setup(fixture.id).unwrap().unwrap();
            for entry in &mut checkpoint.files {
                entry.restoring_from = Some(read_entry(&fixture.root.join(&entry.path)).unwrap());
            }
            checkpoint.phase = WikiSetupPhase::Restoring;
            fixture.store.save_wiki_setup(&checkpoint).unwrap();
            let entry = &checkpoint.files[0];
            replace_if_expected(
                &fixture.root.join(&entry.path),
                entry.restoring_from.as_ref().unwrap(),
                &entry.original,
            )
            .unwrap();
            if edit {
                fixture.write("AGENTS.md", "later user edit");
            }
            let store =
                RepositoryMemoryStore::at_persistent(&fixture.temp.path().join("persistent"))
                    .unwrap();
            let result = restore(&store, fixture.id, &fixture.root, &fixture.source);
            if edit {
                assert!(result.is_err());
                assert_eq!(
                    fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
                    "later user edit"
                );
            } else {
                result.unwrap();
                fixture.restore();
                assert_eq!(
                    fs::read_to_string(fixture.root.join("CLAUDE.md")).unwrap(),
                    CLAUDE
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn interrupted_preparation_keeps_the_original_snapshot() {
        let mut fixture = Fixture::new();
        fs::remove_file(fixture.root.join("CLAUDE.md")).unwrap();
        std::os::unix::fs::symlink("AGENTS.md", fixture.root.join("CLAUDE.md")).unwrap();
        fixture.commit();
        let checkpoint = WikiSetupCheckpoint {
            version: 1,
            workspace_id: fixture.id,
            repository_path: fixture.root.clone(),
            source_commit: fixture.source.clone(),
            phase: WikiSetupPhase::Preparing,
            files: INSTRUCTION_PATHS
                .iter()
                .map(|path| capture_instruction(&fixture.root, path).unwrap())
                .collect(),
        };
        fixture.store.save_wiki_setup(&checkpoint).unwrap();
        let entry = &checkpoint.files[1];
        replace_if_expected(
            &fixture.root.join(&entry.path),
            &entry.original,
            &entry.prepared,
        )
        .unwrap();
        fixture.prepare();
        fixture.restore();
        assert!(
            fs::symlink_metadata(fixture.root.join("CLAUDE.md"))
                .unwrap()
                .is_symlink()
        );
    }

    #[test]
    fn provenance_to_discarded_setup_is_rejected_before_and_after_restore() {
        for resource in [
            "repo://AGENTS.md#L9",
            "repo://%41GENTS.md",
            "repo://./CLAUDE.md",
            "repo://.github/workflows/openwiki-update.yml",
        ] {
            let fixture = Fixture::new();
            fixture.prepare();
            fixture.generate();
            fixture.page(resource);
            assert!(fixture.provenance().is_err(), "{resource}");
            fixture.restore();
            assert!(fixture.provenance().is_err(), "{resource}");
            assert!(fixture.root.join("openwiki/page.md").is_file());
        }
    }

    #[test]
    fn malformed_oversized_and_identity_mismatched_snapshots_fail_closed() {
        let fixture = Fixture::new();
        fixture.write("AGENTS.md", "<!-- OPENWIKI:START -->\nunclosed");
        assert!(preflight(&fixture.root).is_err());
        fixture.write("AGENTS.md", &"x".repeat(MAX_INSTRUCTION_BYTES as usize + 1));
        assert!(preflight(&fixture.root).is_err());
        fixture.write("AGENTS.md", AGENTS);
        fixture.prepare();
        assert!(
            restore(
                &fixture.store,
                fixture.id,
                &fixture.root,
                "different-source"
            )
            .is_err()
        );
        let mut checkpoint = fixture.store.wiki_setup(fixture.id).unwrap().unwrap();
        checkpoint.files[0].path = "../outside.md".into();
        fixture.store.save_wiki_setup(&checkpoint).unwrap();
        assert!(restore(&fixture.store, fixture.id, &fixture.root, &fixture.source).is_err());
    }

    #[test]
    fn legacy_runs_do_not_manufacture_an_original_snapshot() {
        let fixture = Fixture::new();
        fixture.generate();
        fixture.restore();
        assert!(fixture.store.wiki_setup(fixture.id).unwrap().is_none());
        assert!(
            fs::read_to_string(fixture.root.join("AGENTS.md"))
                .unwrap()
                .contains(BLOCK)
        );
        assert!(
            super::super::publication_paths(
                &git::GitService::new(),
                &fixture.root,
                &fixture.source
            )
            .is_err()
        );
    }

    #[test]
    fn evidence_paths_reject_escaping_or_ambiguous_forms() {
        for resource in [
            "repo://../AGENTS.md",
            "repo://%2fAGENTS.md",
            "repo://%2e%2e/AGENTS.md",
            "repo://AGENTS.md?query",
            "repo://AGENTS%5cmd",
            "repo://%00",
        ] {
            assert!(evidence_path(resource).is_err(), "{resource}");
        }
        assert_eq!(
            evidence_path("repo://src/file.rs#L1-L3")
                .unwrap()
                .as_deref(),
            Some("src/file.rs")
        );
    }
}
