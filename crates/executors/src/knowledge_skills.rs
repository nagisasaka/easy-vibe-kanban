use std::path::Path;

use api_types::SelectedSkill;

const RECALL: &str = include_str!("../../../assets/skills/knowledge-recall/SKILL.md");
const ENRICH: &str = include_str!("../../../assets/skills/knowledge-enrich/SKILL.md");
const START: &str = "<!-- vk:pipeline:start -->";
const END: &str = "<!-- vk:pipeline:end -->";
const HEADING: &str = "## Pipeline: LLM Wiki";

pub fn contains_wikillm_block(prompt: &str) -> bool {
    let lines = prompt.lines().collect::<Vec<_>>();
    lines.iter().enumerate().rev().any(|(start, line)| {
        if line.trim_end_matches('\r') != START {
            return false;
        }
        let block = &lines[start + 1..];
        let Some(end) = block
            .iter()
            .position(|candidate| candidate.trim_end_matches('\r') == END)
        else {
            return false;
        };
        block[..end]
            .iter()
            .any(|candidate| candidate.trim_end_matches('\r') == HEADING)
    })
}

fn materialize_into(root: &Path) -> std::io::Result<Vec<SelectedSkill>> {
    let definitions = [("knowledge-recall", RECALL), ("knowledge-enrich", ENRICH)];
    definitions
        .into_iter()
        .map(|(name, content)| {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("SKILL.md");
            std::fs::write(&path, content)?;
            Ok(SelectedSkill {
                name: name.to_string(),
                path,
            })
        })
        .collect()
}

pub fn augment_for_wikillm(prompt: &str, mut selected: Vec<SelectedSkill>) -> Vec<SelectedSkill> {
    if !contains_wikillm_block(prompt) {
        return selected;
    }
    let root = workspace_utils::path::llm_wiki_skills_dir();
    match materialize_into(&root) {
        Ok(bundled) => {
            for skill in bundled {
                if !selected.iter().any(|existing| existing.name == skill.name) {
                    selected.push(skill);
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, path = %root.display(), "failed to prepare LLM Wiki skills")
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn only_recognises_a_complete_standalone_wikillm_block() {
        assert!(!contains_wikillm_block("ordinary task"));
        assert!(!contains_wikillm_block(&format!(
            "quoted {START} {HEADING} {END}"
        )));
        assert!(contains_wikillm_block(&format!(
            "task\n{START}\n{HEADING}\n1. recall\n{END}"
        )));
    }

    #[test]
    fn materialises_both_noop_capable_skills() {
        let temp = tempdir().unwrap();
        let skills = materialize_into(temp.path()).unwrap();
        assert_eq!(skills.len(), 2);
        let recall = std::fs::read_to_string(&skills[0].path).unwrap();
        let enrich = std::fs::read_to_string(&skills[1].path).unwrap();
        assert!(recall.contains("No Wiki and no relevant result are successful"));
        assert!(enrich.contains("successful outcome"));
        assert!(!temp.path().join("PRIOR_KNOWLEDGE.md").exists());
    }
}
