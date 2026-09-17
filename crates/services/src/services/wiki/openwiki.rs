//! Read-only OpenWiki layout adapter for the existing Wiki Viewer.
//! No setup, configuration writes, Claims interpretation or OpenWiki execution.
use super::*;

pub fn load_snapshot(repo_root: &Path) -> Result<WikiSnapshot, WikiError> {
    let repo = fs::canonicalize(repo_root)?;
    let path = repo.join("openwiki");
    let mut snapshot = WikiSnapshot {
        exists: false,
        config: None,
        index: None,
        pages: Vec::new(),
    };
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(snapshot),
        result => {
            result?;
        }
    }
    let root = canonical_child(&repo, &path)?;
    if !root.is_dir() {
        return Err(WikiError::UnsafePath(path.display().to_string()));
    }
    snapshot.exists = true;
    let mut pending = vec![root.clone()];
    let mut total_bytes = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| WikiError::InvalidLayout("Non-UTF-8 Wiki path".into()))?;
            // Private run/Claims records and operator instructions are not pages.
            if name.starts_with('.') || name == "INSTRUCTIONS.md" {
                continue;
            }
            let path = canonical_child(&root, &entry.path())?;
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if snapshot.pages.len() + usize::from(snapshot.index.is_some()) >= MAX_PAGES {
                return Err(WikiError::InvalidLayout(
                    "OpenWiki exceeds Viewer page limit (2000)".into(),
                ));
            }
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| WikiError::UnsafePath(path.display().to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            total_bytes += fs::metadata(&path)?.len() as usize;
            if total_bytes > 16 * 1024 * 1024 {
                return Err(WikiError::InvalidLayout(
                    "OpenWiki exceeds Viewer total content limit (16 MiB)".into(),
                ));
            }
            let page = read_markdown_with(&root, &path, &relative, parse_page)?;
            if relative == "index.md" {
                snapshot.index = Some(page);
            } else {
                snapshot.pages.push(page);
            }
        }
    }
    snapshot.pages.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(snapshot)
}

fn parse_page(path: &str, input: &str) -> Result<WikiPage, WikiError> {
    let text = input.replace("\r\n", "\n");
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok(WikiPage {
            path: path.into(),
            metadata: None,
            content: text,
        });
    };
    let invalid = |message: String| WikiError::InvalidPage {
        path: path.into(),
        message,
    };
    let end = rest
        .find("\n---\n")
        .ok_or_else(|| invalid("unterminated OpenWiki frontmatter".into()))?;
    let value: serde_yaml::Value =
        serde_yaml::from_str(&rest[..end]).map_err(|e| invalid(e.to_string()))?;
    if !value.is_mapping() {
        return Err(invalid("OpenWiki frontmatter must be a mapping".into()));
    }
    let string = |key: &str| value[key].as_str().unwrap_or_default().to_string();
    let strings = |key: &str| {
        value[key]
            .as_sequence()
            .map(|xs| {
                xs.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let metadata = value["title"].as_str().map(|title| WikiPageMetadata {
        schema_version: 1,
        title: title.into(),
        summary: string("description"),
        language: string("language"),
        tags: strings("tags"),
        sources: strings("sources"),
        repos: Vec::new(),
        created: String::new(),
        updated: String::new(),
    });
    Ok(WikiPage {
        path: path.into(),
        metadata,
        content: rest[end + 5..].into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_nested_okf_without_legacy_configuration_or_mutation() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!load_snapshot(temp.path()).unwrap().exists);
        fs::create_dir_all(temp.path().join("openwiki/architecture")).unwrap();
        fs::create_dir_all(temp.path().join("openwiki/.claims")).unwrap();
        fs::write(temp.path().join("openwiki/.claims/hidden.md"), "private").unwrap();
        fs::write(temp.path().join("openwiki/INSTRUCTIONS.md"), "instructions").unwrap();
        fs::write(
            temp.path().join("openwiki/index.md"),
            "# Wiki\n[System](architecture/system.md)",
        )
        .unwrap();
        let page = "---\r\ntype: architecture\r\ntitle: 境界\r\ndescription: 根拠\r\ntags: [runtime]\r\nverified: [{by: openwiki/0.5.1, at: today}]\r\n---\r\n# 境界\r\n本文";
        let path = temp.path().join("openwiki/architecture/system.md");
        fs::write(&path, page).unwrap();
        let snapshot = load_snapshot(temp.path()).unwrap();
        assert_eq!(snapshot.pages.len(), 1);
        assert_eq!(snapshot.pages[0].path, "architecture/system.md");
        assert_eq!(snapshot.pages[0].metadata.as_ref().unwrap().title, "境界");
        assert_eq!(snapshot.pages[0].metadata.as_ref().unwrap().summary, "根拠");
        assert!(snapshot.config.is_none());
        assert_eq!(fs::read_to_string(path).unwrap(), page);
        assert!(!temp.path().join(".llm-wiki").exists());
        assert!(parse_page("bad.md", "---\nx: [\n---\nbody").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_external_or_dangling_wiki_and_nested_links() {
        let temp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let wiki = temp.path().join("openwiki");
        std::os::unix::fs::symlink(other.path().join("missing"), &wiki).unwrap();
        assert!(load_snapshot(temp.path()).is_err());
        fs::remove_file(&wiki).unwrap();
        fs::create_dir(&wiki).unwrap();
        std::os::unix::fs::symlink(other.path(), wiki.join("nested")).unwrap();
        assert!(load_snapshot(temp.path()).is_err());
    }

    #[test]
    fn existing_legacy_files_are_ignored_and_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join(".llm-wiki");
        fs::create_dir(&legacy).unwrap();
        fs::write(
            legacy.join("config.toml"),
            "invalid legacy config, retained",
        )
        .unwrap();
        fs::write(legacy.join("index.md"), "User knowledge").unwrap();
        assert!(!load_snapshot(temp.path()).unwrap().exists);
        fs::create_dir(temp.path().join("openwiki")).unwrap();
        fs::write(temp.path().join("openwiki/index.md"), "# Canonical").unwrap();
        assert_eq!(
            load_snapshot(temp.path()).unwrap().index.unwrap().content,
            "# Canonical"
        );
        assert_eq!(
            fs::read_to_string(legacy.join("config.toml")).unwrap(),
            "invalid legacy config, retained"
        );
        assert_eq!(
            fs::read_to_string(legacy.join("index.md")).unwrap(),
            "User knowledge"
        );
    }

    #[test]
    fn rejects_oversized_pages_instead_of_returning_a_partial_wiki() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("openwiki")).unwrap();
        fs::write(temp.path().join("openwiki/index.md"), "# Index").unwrap();
        fs::write(
            temp.path().join("openwiki/large.md"),
            vec![b'a'; MAX_PAGE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(load_snapshot(temp.path()).is_err());
    }
}
