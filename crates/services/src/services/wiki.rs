//! Shared, read-only Wiki display types and safe file reading.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

pub mod openwiki;
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
    #[error("invalid Wiki layout: {0}")]
    InvalidLayout(String),
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

fn read_markdown_with(
    root: &Path,
    path: &Path,
    display: &str,
    parse: fn(&str, &str) -> Result<WikiPage, WikiError>,
) -> Result<WikiPage, WikiError> {
    let canonical = canonical_child(root, path)?;
    let metadata = fs::metadata(&canonical)?;
    if !metadata.is_file() || metadata.len() > MAX_PAGE_BYTES {
        return Err(WikiError::UnsafePath(display.to_string()));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(canonical)?;
    if !file.metadata()?.is_file() {
        return Err(WikiError::UnsafePath(display.to_string()));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PAGE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(WikiError::UnsafePath(display.to_string()));
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    parse(display, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_language_tags_for_repository_memory() {
        for valid in ["en", "ja", "pt-BR", "zh-Hans", "x-private"] {
            assert!(is_valid_language_tag(valid));
        }
        for invalid in ["", "j", "../ja", "en_Us", "en-", "x"] {
            assert!(!is_valid_language_tag(invalid));
        }
    }
}
