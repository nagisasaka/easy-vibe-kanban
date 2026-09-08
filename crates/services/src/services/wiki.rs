//! Repository-local LLM Wiki storage and parsing.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

pub const WIKI_DIR: &str = ".llm-wiki";
pub const DEFAULT_OUTPUT_LANGUAGE: &str = "en";
const MAX_PAGE_BYTES: u64 = 512 * 1024;
const MAX_PAGES: usize = 2_000;

#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct WikiConfig {
    pub version: u32,
    pub output_language: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct WikiPageMetadata {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub title: String,
    pub language: String,
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub repos: Vec<String>,
    pub created: String,
    pub updated: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct WikiPage {
    pub path: String,
    pub metadata: Option<WikiPageMetadata>,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct WikiSnapshot {
    pub exists: bool,
    pub config: Option<WikiConfig>,
    pub index: Option<WikiPage>,
    pub pages: Vec<WikiPage>,
}

#[derive(Debug, Error)]
pub enum WikiError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid Wiki configuration: {0}")]
    InvalidConfig(String),
    #[error("invalid Wiki page {path}: {message}")]
    InvalidPage { path: String, message: String },
    #[error("Wiki path is unsafe: {0}")]
    UnsafePath(String),
}

fn default_schema_version() -> u32 {
    1
}

pub fn is_valid_language_tag(value: &str) -> bool {
    if value.len() < 2 || value.len() > 63 || value.starts_with('-') || value.ends_with('-') {
        return false;
    }
    let parts = value.split('-').collect::<Vec<_>>();
    let first = parts[0];
    let private_use = first.eq_ignore_ascii_case("x");
    if (!private_use
        && (!(2..=8).contains(&first.len()) || !first.chars().all(|c| c.is_ascii_alphabetic())))
        || (private_use && parts.len() == 1)
    {
        return false;
    }
    parts[1..].iter().all(|part| {
        (1..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

pub fn parse_config(input: &str) -> Result<WikiConfig, WikiError> {
    let config: WikiConfig =
        toml::from_str(input).map_err(|error| WikiError::InvalidConfig(error.to_string()))?;
    if config.version != 1 {
        return Err(WikiError::InvalidConfig(format!(
            "unsupported version {}",
            config.version
        )));
    }
    if !is_valid_language_tag(&config.output_language) {
        return Err(WikiError::InvalidConfig(
            "output_language must be a BCP 47 language tag".to_string(),
        ));
    }
    Ok(config)
}

pub fn serialize_config(output_language: &str) -> Result<String, WikiError> {
    if !is_valid_language_tag(output_language) {
        return Err(WikiError::InvalidConfig(
            "output_language must be a BCP 47 language tag".to_string(),
        ));
    }
    Ok(format!(
        "version = 1\noutput_language = {:?}\n",
        output_language
    ))
}

pub fn parse_page(path: &str, input: &str) -> Result<WikiPage, WikiError> {
    let (metadata, content) = if let Some(rest) = input.strip_prefix("---\n") {
        let Some(end) = rest.find("\n---\n") else {
            return Err(WikiError::InvalidPage {
                path: path.to_string(),
                message: "unterminated YAML frontmatter".to_string(),
            });
        };
        let yaml = &rest[..end];
        let metadata = serde_yaml::from_str::<WikiPageMetadata>(yaml).map_err(|error| {
            WikiError::InvalidPage {
                path: path.to_string(),
                message: error.to_string(),
            }
        })?;
        if metadata.schema_version != 1 {
            return Err(WikiError::InvalidPage {
                path: path.to_string(),
                message: format!("unsupported schema_version {}", metadata.schema_version),
            });
        }
        if !is_valid_language_tag(&metadata.language) {
            return Err(WikiError::InvalidPage {
                path: path.to_string(),
                message: "language must be a BCP 47 language tag".to_string(),
            });
        }
        (Some(metadata), rest[end + 5..].to_string())
    } else {
        (None, input.to_string())
    };
    Ok(WikiPage {
        path: path.to_string(),
        metadata,
        content,
    })
}

fn canonical_child(root: &Path, child: &Path) -> Result<PathBuf, WikiError> {
    let metadata = fs::symlink_metadata(child)?;
    if metadata.file_type().is_symlink() {
        return Err(WikiError::UnsafePath(child.display().to_string()));
    }
    let canonical = fs::canonicalize(child)?;
    if !canonical.starts_with(root) {
        return Err(WikiError::UnsafePath(child.display().to_string()));
    }
    Ok(canonical)
}

fn read_markdown(root: &Path, path: &Path, display: &str) -> Result<WikiPage, WikiError> {
    let canonical = canonical_child(root, path)?;
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() || metadata.len() > MAX_PAGE_BYTES {
        return Err(WikiError::UnsafePath(display.to_string()));
    }
    parse_page(display, &fs::read_to_string(canonical)?)
}

pub fn load_snapshot(repo_root: &Path) -> Result<WikiSnapshot, WikiError> {
    let repo_root = fs::canonicalize(repo_root)?;
    let wiki_path = repo_root.join(WIKI_DIR);
    if !wiki_path.exists() {
        return Ok(WikiSnapshot {
            exists: false,
            config: None,
            index: None,
            pages: Vec::new(),
        });
    }
    let wiki_root = canonical_child(&repo_root, &wiki_path)?;
    if !wiki_root.is_dir() {
        return Err(WikiError::UnsafePath(wiki_path.display().to_string()));
    }

    let config_path = wiki_root.join("config.toml");
    let config = if config_path.exists() {
        let config_path = canonical_child(&wiki_root, &config_path)?;
        Some(parse_config(&fs::read_to_string(config_path)?)?)
    } else {
        None
    };
    let index_path = wiki_root.join("index.md");
    let index = if index_path.exists() {
        Some(read_markdown(&wiki_root, &index_path, "index.md")?)
    } else {
        None
    };

    let mut pages = Vec::new();
    let pages_path = wiki_root.join("pages");
    if pages_path.exists() {
        let pages_root = canonical_child(&wiki_root, &pages_path)?;
        if !pages_root.is_dir() {
            return Err(WikiError::UnsafePath(pages_path.display().to_string()));
        }
        for entry in fs::read_dir(&pages_root)? {
            if pages.len() >= MAX_PAGES {
                break;
            }
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            pages.push(read_markdown(&pages_root, &path, &format!("pages/{name}"))?);
        }
    }
    pages.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(WikiSnapshot {
        exists: true,
        config,
        index,
        pages,
    })
}

fn ensure_plain_directory(parent_root: &Path, path: &Path) -> Result<PathBuf, WikiError> {
    if path.exists() {
        return canonical_child(parent_root, path);
    }
    fs::create_dir(path)?;
    canonical_child(parent_root, path)
}

pub fn initialise_or_update(repo_root: &Path, output_language: &str) -> Result<(), WikiError> {
    let config = serialize_config(output_language)?;
    let repo_root = fs::canonicalize(repo_root)?;
    let wiki_root = ensure_plain_directory(&repo_root, &repo_root.join(WIKI_DIR))?;
    if !wiki_root.is_dir() {
        return Err(WikiError::UnsafePath(wiki_root.display().to_string()));
    }
    let pages = ensure_plain_directory(&wiki_root, &wiki_root.join("pages"))?;
    if !pages.is_dir() {
        return Err(WikiError::UnsafePath(pages.display().to_string()));
    }

    let index_path = wiki_root.join("index.md");
    if index_path.exists() {
        canonical_child(&wiki_root, &index_path)?;
    }

    let config_path = wiki_root.join("config.toml");
    if config_path.exists() {
        canonical_child(&wiki_root, &config_path)?;
    }
    let temp_path = wiki_root.join(".config.toml.tmp");
    if temp_path.exists() {
        let metadata = fs::symlink_metadata(&temp_path)?;
        if metadata.file_type().is_symlink() {
            return Err(WikiError::UnsafePath(temp_path.display().to_string()));
        }
    }
    fs::write(&temp_path, config)?;
    fs::rename(&temp_path, &config_path)?;

    if !index_path.exists() {
        fs::write(index_path, "# LLM Wiki\n")?;
    }
    Ok(())
}

pub fn search_snapshot<'a>(snapshot: &'a WikiSnapshot, query: &str) -> Vec<&'a WikiPage> {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return snapshot.index.iter().chain(snapshot.pages.iter()).collect();
    }
    snapshot
        .index
        .iter()
        .chain(snapshot.pages.iter())
        .filter(|page| {
            let metadata = page
                .metadata
                .as_ref()
                .map(|metadata| {
                    format!(
                        "{} {} {} {} {}",
                        metadata.title,
                        metadata.summary,
                        metadata.tags.join(" "),
                        metadata.sources.join(" "),
                        metadata.repos.join(" ")
                    )
                })
                .unwrap_or_default();
            let haystack = format!("{} {} {}", page.path, metadata, page.content).to_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

pub fn normalise_page_link(value: &str) -> Option<String> {
    let clean = value.split(['#', '?']).next()?.replace('\\', "/");
    let path = Path::new(&clean);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    let mut display = clean.trim_start_matches("./").to_string();
    if !display.ends_with(".md") {
        display.push_str(".md");
    }
    if !display.contains('/') {
        display = format!("pages/{display}");
    }
    (display == "index.md" || display.starts_with("pages/")).then_some(display)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn validates_and_round_trips_language_config() {
        for tag in ["en", "ja", "pt-BR", "zh-Hant-TW", "x-team"] {
            assert_eq!(
                parse_config(&serialize_config(tag).unwrap())
                    .unwrap()
                    .output_language,
                tag
            );
        }
        for invalid in ["", "j", "../ja", "ja_JP", "日本語"] {
            assert!(!is_valid_language_tag(invalid));
        }
    }

    #[test]
    fn initialises_deterministically_and_preserves_pages() {
        let temp = tempdir().unwrap();
        initialise_or_update(temp.path(), DEFAULT_OUTPUT_LANGUAGE).unwrap();
        initialise_or_update(temp.path(), DEFAULT_OUTPUT_LANGUAGE).unwrap();
        let snapshot = load_snapshot(temp.path()).unwrap();
        assert!(snapshot.exists);
        assert_eq!(snapshot.config.unwrap().output_language, "en");
        assert_eq!(snapshot.index.unwrap().content, "# LLM Wiki\n");
    }

    #[test]
    fn parses_frontmatter_and_searches_metadata_and_body() {
        let page = parse_page(
            "pages/pipeline.md",
            "---\nschema_version: 1\ntitle: Pipeline\nlanguage: ja\nsummary: 再合成\ntags: [pipeline]\nsources: [EASY-123]\nrepos: [easy]\ncreated: '2026-09-09'\nupdated: '2026-09-09'\n---\n本文",
        )
        .unwrap();
        let snapshot = WikiSnapshot {
            exists: true,
            config: None,
            index: None,
            pages: vec![page],
        };
        assert_eq!(search_snapshot(&snapshot, "EASY-123 再合成").len(), 1);
        assert!(search_snapshot(&snapshot, "missing").is_empty());
    }

    #[test]
    fn rejects_traversal_and_wiki_symlinks() {
        assert_eq!(normalise_page_link("../secret.md"), None);
        assert_eq!(normalise_page_link("topic"), Some("pages/topic.md".into()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let temp = tempdir().unwrap();
            let outside = tempdir().unwrap();
            symlink(outside.path(), temp.path().join(WIKI_DIR)).unwrap();
            assert!(matches!(
                load_snapshot(temp.path()),
                Err(WikiError::UnsafePath(_))
            ));
            assert!(matches!(
                initialise_or_update(temp.path(), "en"),
                Err(WikiError::UnsafePath(_))
            ));
        }
    }
}
