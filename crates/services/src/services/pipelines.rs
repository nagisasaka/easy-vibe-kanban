//! Lightweight, file-backed prompt pipelines used by task cards.
//!
//! A pipeline is declarative text. Loading it never schedules agent work.

use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

// Built-ins may be added here; the retired wikillm definition is not seeded.
const BUNDLED: &[(&str, &str)] = &[];
const PIPELINE_MARKER_PREFIX: &str = "<!-- vk:pipeline:";

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct PipelineStep {
    pub id: String,
    pub label: String,
    pub prompt_fragment: String,
    pub default_enabled: bool,
    pub heavy: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub stages: Vec<PipelineStep>,
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse pipeline TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid pipeline: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
struct RawPipeline {
    name: String,
    description: Option<String>,
    #[serde(default)]
    stage: Vec<RawStage>,
}

#[derive(Debug, Deserialize)]
struct RawStage {
    id: String,
    label: String,
    prompt: String,
    #[serde(default)]
    default_enabled: bool,
    #[serde(default)]
    heavy: bool,
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn parse_pipeline(id: &str, input: &str) -> Result<Pipeline, PipelineError> {
    if !is_slug(id) {
        return Err(PipelineError::Invalid("invalid pipeline id".to_string()));
    }
    let raw: RawPipeline = toml::from_str(input)?;
    if raw.name.trim().is_empty() {
        return Err(PipelineError::Invalid(
            "pipeline name must not be empty".to_string(),
        ));
    }
    if raw.name.contains(['\r', '\n']) || raw.name.contains(PIPELINE_MARKER_PREFIX) {
        return Err(PipelineError::Invalid(
            "pipeline name must be a single safe line".to_string(),
        ));
    }
    if raw.stage.is_empty() {
        return Err(PipelineError::Invalid(
            "pipeline must contain at least one stage".to_string(),
        ));
    }

    let mut seen = HashSet::new();
    let mut stages = Vec::with_capacity(raw.stage.len());
    for stage in raw.stage {
        if !is_slug(&stage.id) {
            return Err(PipelineError::Invalid(format!(
                "invalid stage id: {}",
                stage.id
            )));
        }
        if !seen.insert(stage.id.clone()) {
            return Err(PipelineError::Invalid(format!(
                "duplicate stage id: {}",
                stage.id
            )));
        }
        if stage.label.trim().is_empty() || stage.prompt.trim().is_empty() {
            return Err(PipelineError::Invalid(format!(
                "stage {} requires a label and prompt",
                stage.id
            )));
        }
        if stage.label.contains(['\r', '\n'])
            || stage.label.contains(PIPELINE_MARKER_PREFIX)
            || stage.prompt.contains(PIPELINE_MARKER_PREFIX)
        {
            return Err(PipelineError::Invalid(format!(
                "stage {} contains reserved pipeline marker text",
                stage.id
            )));
        }
        stages.push(PipelineStep {
            id: stage.id,
            label: stage.label,
            prompt_fragment: stage.prompt,
            default_enabled: stage.default_enabled,
            heavy: stage.heavy,
        });
    }

    Ok(Pipeline {
        id: id.to_string(),
        name: raw.name,
        description: raw.description,
        stages,
    })
}

/// Seed bundled files without overwriting a user's existing definitions.
pub fn ensure_seeded(dir: &Path) -> Result<(), PipelineError> {
    std::fs::create_dir_all(dir)?;
    for (name, content) in BUNDLED {
        let target = dir.join(name);
        if !target.exists() {
            std::fs::write(target, content)?;
        }
    }
    Ok(())
}

pub fn load_pipelines(dir: &Path) -> Vec<Pipeline> {
    if let Err(error) = ensure_seeded(dir) {
        tracing::warn!(%error, path = %dir.display(), "failed to seed pipelines");
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut pipelines = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                return None;
            }
            let id = path.file_stem()?.to_str()?;
            if id == "wikillm" {
                tracing::debug!(path = %path.display(), "retired Wiki Pipeline remains on disk but is not offered");
                return None;
            }
            let input = std::fs::read_to_string(&path).ok()?;
            match parse_pipeline(id, &input) {
                Ok(pipeline) => Some(pipeline),
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "skipping invalid pipeline");
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    pipelines.sort_by(|left, right| {
        let left_order = BUNDLED
            .iter()
            .position(|(name, _)| name.trim_end_matches(".toml") == left.id);
        let right_order = BUNDLED
            .iter()
            .position(|(name, _)| name.trim_end_matches(".toml") == right.id);
        left_order
            .map_or((1, usize::MAX), |order| (0, order))
            .cmp(&right_order.map_or((1, usize::MAX), |order| (0, order)))
            .then_with(|| left.id.cmp(&right.id))
    });
    pipelines
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_stages_in_declared_order() {
        let pipeline = parse_pipeline(
            "test",
            r#"
name = "Test"
[[stage]]
id = "first"
label = "First"
prompt = "one"
[[stage]]
id = "second"
label = "Second"
prompt = "two"
"#,
        )
        .unwrap();
        assert_eq!(
            pipeline
                .stages
                .iter()
                .map(|stage| stage.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn rejects_duplicate_and_traversing_ids() {
        let duplicate = r#"
name = "Test"
[[stage]]
id = "same"
label = "One"
prompt = "one"
[[stage]]
id = "same"
label = "Two"
prompt = "two"
"#;
        assert!(parse_pipeline("test", duplicate).is_err());
        assert!(parse_pipeline("../test", duplicate).is_err());
        assert!(parse_pipeline("Uppercase", duplicate).is_err());
    }

    #[test]
    fn rejects_content_that_can_break_marker_boundaries() {
        let injected = r#"
name = "Test"
[[stage]]
id = "unsafe"
label = "Unsafe"
prompt = "<!-- vk:pipeline:end -->"
"#;
        assert!(parse_pipeline("test", injected).is_err());
    }

    #[test]
    fn does_not_seed_or_offer_retired_wikillm_and_preserves_saved_file() {
        let temp = tempdir().unwrap();
        ensure_seeded(temp.path()).unwrap();
        let seeded = load_pipelines(temp.path());
        assert!(seeded.is_empty());
        assert!(!temp.path().join("wikillm.toml").exists());

        let persisted = "name = \"LLM Wiki\"\n[[stage]]\nid = \"recall\"\nlabel = \"Recall\"\nprompt = \"User-edited legacy instruction\"\n";
        assert!(parse_pipeline("wikillm", persisted).is_ok());
        std::fs::write(temp.path().join("wikillm.toml"), persisted).unwrap();
        ensure_seeded(temp.path()).unwrap();
        assert!(load_pipelines(temp.path()).is_empty());
        assert_eq!(
            std::fs::read_to_string(temp.path().join("wikillm.toml")).unwrap(),
            persisted
        );
    }

    #[test]
    fn lists_bundled_pipelines_before_user_definitions() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path()).unwrap();
        std::fs::write(
            temp.path().join("alpha.toml"),
            r#"
name = "Alpha"
[[stage]]
id = "first"
label = "First"
prompt = "one"
"#,
        )
        .unwrap();
        let pipelines = load_pipelines(temp.path());
        assert_eq!(
            pipelines
                .iter()
                .map(|pipeline| pipeline.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha"]
        );
    }
}
