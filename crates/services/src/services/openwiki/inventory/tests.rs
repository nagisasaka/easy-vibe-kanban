use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    store: RepositoryMemoryStore,
    identity: InventoryIdentity,
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
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
    String::from_utf8(output.stdout).unwrap().trim().into()
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Fixture"]);
        git(&root, &["config", "user.email", "fixture@example.invalid"]);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "--allow-empty", "-m", "fixture source"]);
        let source = git(&root, &["rev-parse", "HEAD"]);
        let persistent = temp.path().join("persistent");
        std::fs::create_dir(&persistent).unwrap();
        let store = RepositoryMemoryStore::at_persistent(&persistent).unwrap();
        Self {
            _temp: temp,
            root,
            store,
            identity: InventoryIdentity::new(
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                source,
            ),
        }
    }
    fn generate(&self) -> DocumentInventory {
        DocumentInventory::generate(
            &GitService::new(),
            &self.root,
            &self.store,
            self.identity.clone(),
        )
        .unwrap()
    }
    fn documents(&self, inventory: &DocumentInventory) -> Vec<DocumentEntry> {
        let DocumentInventory::Available { chunks, .. } = inventory else {
            return Vec::new();
        };
        (0..*chunks)
            .flat_map(|n| {
                serde_json::from_slice::<Chunk>(
                    &self
                        .store
                        .read_document_inventory(self.identity.run_id, Some(n))
                        .unwrap(),
                )
                .unwrap()
                .entries
            })
            .collect()
    }
}

#[test]
fn markdown_source_positions_frontmatter_code_and_duplicates() {
    let source = "---\r\ntitle: Hidden\r\n---\r\nIntro\r\n\r\n# 日本語 *Title*\r\n\r\nSecond\r\n------\r\n\r\n```md\r\n# fake\r\n```\r\n\r\n## Second\r\n";
    let headings = parse_headings(source, false).unwrap();
    assert_eq!(
        headings
            .iter()
            .map(|h| (h.depth, h.text.as_str(), h.line))
            .collect::<Vec<_>>(),
        [(1, "日本語 Title", 6), (2, "Second", 8), (2, "Second", 15)]
    );
    assert_eq!(headings[0].next_heading_line, Some(8));
    assert_eq!(headings[2].next_heading_line, None);
    assert!(
        parse_headings("intro without headings", false)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn mdx_static_jsx_children_and_positions() {
    let headings = parse_headings("---\r\ntitle: Test\r\n---\r\n# 日本語\r\n\r\n<Tabs>\r\n\r\n## Inner\r\n\r\n</Tabs>\r\n\r\n<h2>Not a Markdown heading</h2>\r\n", true).unwrap();
    assert_eq!(
        headings
            .iter()
            .map(|h| (h.text.as_str(), h.line))
            .collect::<Vec<_>>(),
        [("日本語", 4), ("Inner", 8)]
    );
}

#[test]
fn mdx_never_misinterprets_or_executes_javascript() {
    for text in [
        "export const text = `\n\n# not a heading\n\n`;\n\n# real",
        "import x from './side-effect.js'\n\n# real",
        "{(() => { throw new Error('must not run'); })()}\n\n# real",
        "# {process.env.SECRET}",
        "{/* # fake */}\n\n# real",
        "<Broken>\n\n# heading\n",
    ] {
        assert!(parse_headings(text, true).is_err(), "{text}");
    }
    let headings =
        parse_headings("```js\nimport 'side-effect';\n# fake\n```\n\n# real", true).unwrap();
    assert_eq!(headings.len(), 1);
    assert_eq!(headings[0].text, "real");
}

#[test]
fn inventory_zero_other_formats_mdx_and_instruction_boundaries() {
    for files in [
        vec![],
        vec![("guide.pdf", "not read"), ("README", "not read")],
        vec![("guide.MDX", "# API")],
    ] {
        let f = Fixture::new(&files);
        let inventory = f.generate();
        inventory.validate(&f.store, &f.identity).unwrap();
        assert_eq!(f.documents(&inventory).len(), files.len());
        assert!(
            inventory
                .prompt(&f.store, true)
                .contains("independent source/tests/config")
        );
    }
    let f = Fixture::new(&[
        ("AGENTS.md", "# forbidden"),
        ("nested/CLAUDE.md", "# forbidden"),
        ("openwiki/index.md", "# generated"),
        ("guide.md", "# Guide"),
        ("archive/old.md", "# Historic"),
        ("bad.mdx", "<Broken>"),
        ("intro.md", "Intro only"),
    ]);
    std::fs::write(f.root.join("untracked.md"), "# invisible").unwrap();
    std::fs::write(f.root.join("AGENTS.md"), "# setup replacement").unwrap();
    let inventory = f.generate();
    let docs = f.documents(&inventory);
    assert_eq!(docs.len(), 6);
    assert_eq!(
        docs.iter()
            .filter(|d| d.status == EntryStatus::InstructionOnly)
            .count(),
        2
    );
    assert!(
        docs.iter()
            .any(|d| d.path == "archive/old.md" && !d.headings.is_empty())
    );
    assert!(
        docs.iter()
            .any(|d| d.path == "bad.mdx" && d.reason.is_some() && d.headings.is_empty())
    );
    assert!(
        docs.iter()
            .any(|d| d.path == "intro.md" && d.reason.as_deref() == Some("no_static_headings"))
    );
}

#[test]
fn ignore_presence_is_unavailable_not_empty_and_mismatch_fails() {
    for text in ["", "# comment", "docs/**"] {
        let f = Fixture::new(&[(".openwikiignore", text), ("guide.md", "# hidden")]);
        let inventory = f.generate();
        assert!(matches!(
            inventory,
            DocumentInventory::UnavailableIgnorePolicy { .. }
        ));
        assert!(
            !f.store
                .document_inventory_path(f.identity.run_id, None)
                .exists()
        );
        assert!(
            inventory
                .prompt(&f.store, false)
                .contains("unknown, not zero")
        );
        assert!(
            !inventory
                .prompt(&f.store, true)
                .contains("read the complete")
        );
        std::fs::write(f.root.join(".openwikiignore"), "different").unwrap();
        assert!(
            DocumentInventory::generate(&GitService::new(), &f.root, &f.store, f.identity.clone())
                .is_err()
        );
    }
    let f = Fixture::new(&[]);
    std::fs::write(f.root.join(".openwikiignore"), "").unwrap();
    assert!(
        DocumentInventory::generate(&GitService::new(), &f.root, &f.store, f.identity.clone())
            .is_err()
    );
}

#[test]
fn positions_downgrade_and_large_documents_remain_visible() {
    let large = "# x\n".repeat(MAX_SOURCE_BYTES / 4 + 1);
    let f = Fixture::new(&[("guide.md", "# original\n"), ("huge.md", &large)]);
    std::fs::write(f.root.join("guide.md"), "\n# transformed\n").unwrap();
    let docs = f.documents(&f.generate());
    assert_eq!(docs.len(), 2);
    assert!(
        docs.iter()
            .all(|d| d.headings.is_empty() && d.reason.is_some())
    );
    assert!(
        docs.iter()
            .any(|d| d.reason.as_deref() == Some("snapshot_worktree_content_mismatch"))
    );
    assert!(
        docs.iter()
            .any(|d| d.reason.as_deref() == Some("source_size_limit"))
    );
}

#[test]
fn chunks_are_complete_deterministic_and_tamper_evident() {
    let paths: Vec<_> = (0..650).map(|n| format!("docs/{n:04}.md")).collect();
    let files: Vec<_> = paths
        .iter()
        .map(|path| (path.as_str(), "# Heading\n\n## Second\n"))
        .collect();
    let f = Fixture::new(&files);
    let inventory = f.generate();
    assert_eq!(f.documents(&inventory).len(), 650);
    assert_eq!(f.generate(), inventory);
    let DocumentInventory::Available { chunks, .. } = &inventory else {
        panic!()
    };
    assert!(*chunks > 1);
    assert!(inventory.prompt(&f.store, true).len() < 4000);
    for changed in ["run", "repo", "workspace", "source"] {
        let mut id = f.identity.clone();
        match changed {
            "run" => id.run_id = Uuid::new_v4(),
            "repo" => id.repository_id = Uuid::new_v4(),
            "workspace" => id.workspace_id = Uuid::new_v4(),
            _ => id.source_sha = "a".repeat(40),
        }
        assert!(inventory.validate(&f.store, &id).is_err());
    }
    let path = f.store.document_inventory_path(f.identity.run_id, Some(0));
    let original = std::fs::read(&path).unwrap();
    let changed = String::from_utf8(original.clone())
        .unwrap()
        .replace("Heading", "Tampered");
    std::fs::write(&path, changed).unwrap();
    assert!(inventory.validate(&f.store, &f.identity).is_err());
    std::fs::write(&path, &original).unwrap();
    inventory.validate(&f.store, &f.identity).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(inventory.validate(&f.store, &f.identity).is_err());
}

#[test]
#[cfg(unix)]
fn symlinks_are_not_followed_for_documents_ignore_or_stored_chunks() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new(&[("guide.md", "# original")]);
    symlink("/no/such/external.md", f.root.join("link.md")).unwrap();
    git(&f.root, &["add", "."]);
    git(&f.root, &["commit", "-m", "link"]);
    f.identity.source_sha = git(&f.root, &["rev-parse", "HEAD"]);
    let inventory = f.generate();
    assert!(
        f.documents(&inventory).iter().any(|d| d.path == "link.md"
            && d.reason.as_deref() == Some("symlink_or_submodule_not_followed"))
    );
    let path = f.store.document_inventory_path(f.identity.run_id, Some(0));
    let copy = f._temp.path().join("external.json");
    std::fs::copy(&path, &copy).unwrap();
    std::fs::remove_file(&path).unwrap();
    symlink(copy, path).unwrap();
    assert!(inventory.validate(&f.store, &f.identity).is_err());
    symlink("guide.md", f.root.join(".openwikiignore")).unwrap();
    assert!(
        DocumentInventory::generate(&GitService::new(), &f.root, &f.store, f.identity.clone())
            .is_err()
    );
}
