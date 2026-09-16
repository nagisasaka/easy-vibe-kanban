//! Full validated phase reports in repository shared storage. NodeExecution
//! carries only the host-owned identity/digest and deterministic routing data.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use utils::repository_memory::{BootstrapReportKind, RepositoryMemoryStore};
use uuid::Uuid;

use super::{CoverageReview, CoverageVerdict, FindingSeverity, RefinementReport};
use crate::services::openwiki::inventory::InventoryIdentity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportIdentity {
    pub source: InventoryIdentity,
    pub phase: BootstrapReportKind,
    pub session_id: Uuid,
    pub agent_run_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredReport {
    identity: ReportIdentity,
    report: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportReference {
    pub report_version: u32,
    pub identity: ReportIdentity,
    pub digest: String,
    pub verdict: Option<CoverageVerdict>,
    pub finding_count: usize,
    pub material_count: usize,
}

impl ReportReference {
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        ensure!(raw.len() <= 4096, "Bootstrap report reference is too large");
        let reference: Self = serde_json::from_str(raw).context("Invalid report reference")?;
        ensure!(
            reference.report_version == 1 && reference.identity.source.version == 1,
            "Unsupported report reference version"
        );
        ensure!(
            reference.digest.len() == 71
                && reference.digest.starts_with("sha256:")
                && reference.digest[7..].bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid report digest"
        );
        ensure!(
            reference.material_count <= reference.finding_count,
            "Invalid finding counts"
        );
        ensure!(
            reference.verdict.is_some()
                == (reference.identity.phase == BootstrapReportKind::Review),
            "Report phase/verdict mismatch"
        );
        if let Some(verdict) = reference.verdict {
            ensure!(
                (reference.material_count > 0) == (verdict == CoverageVerdict::NeedsRefinement),
                "Report counts/verdict mismatch"
            );
        }
        Ok(reference)
    }

    /// The file location is derived from trusted IDs, never an agent-supplied path.
    pub fn load(
        &self,
        store: &RepositoryMemoryStore,
        expected: &ReportIdentity,
    ) -> anyhow::Result<Value> {
        ensure!(
            &self.identity == expected,
            "Bootstrap report belongs to another phase/run/source"
        );
        let bytes = store.read_bootstrap_report(expected.source.run_id, expected.phase)?;
        ensure!(
            digest(&bytes) == self.digest,
            "Bootstrap report digest mismatch"
        );
        let stored: StoredReport = serde_json::from_slice(&bytes)?;
        ensure!(
            stored.identity == *expected,
            "Stored Bootstrap report identity mismatch"
        );
        Ok(stored.report)
    }

    pub fn load_review(
        &self,
        store: &RepositoryMemoryStore,
        expected: &ReportIdentity,
    ) -> anyhow::Result<CoverageReview> {
        ensure!(
            expected.phase == BootstrapReportKind::Review,
            "Not a Review report"
        );
        let review = CoverageReview::parse(&self.load(store, expected)?.to_string())?;
        ensure!(
            self.verdict == Some(review.verdict)
                && self.finding_count == review.findings.len()
                && self.material_count == material_count(&review),
            "Report routing metadata mismatch"
        );
        Ok(review)
    }

    pub fn load_refinement(
        &self,
        store: &RepositoryMemoryStore,
        expected: &ReportIdentity,
        review: &CoverageReview,
    ) -> anyhow::Result<RefinementReport> {
        ensure!(
            expected.phase == BootstrapReportKind::Refine,
            "Not a Refine report"
        );
        let report = RefinementReport::parse(&self.load(store, expected)?.to_string(), review)?;
        ensure!(
            self.finding_count == report.resolutions.len() && self.material_count == 0,
            "Refine report metadata mismatch"
        );
        Ok(report)
    }

    pub fn prompt_path(&self, store: &RepositoryMemoryStore) -> std::path::PathBuf {
        store.bootstrap_report_path(self.identity.source.run_id, self.identity.phase)
    }
}

fn material_count(review: &CoverageReview) -> usize {
    review
        .findings
        .iter()
        .filter(|f| f.severity == FindingSeverity::Material)
        .count()
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn save(
    store: &RepositoryMemoryStore,
    identity: ReportIdentity,
    report: Value,
    verdict: Option<CoverageVerdict>,
    finding_count: usize,
    material_count: usize,
) -> anyhow::Result<ReportReference> {
    let stored = StoredReport {
        identity: identity.clone(),
        report,
    };
    let reference = ReportReference {
        report_version: 1,
        identity: identity.clone(),
        digest: digest(&serde_json::to_vec_pretty(&stored)?),
        verdict,
        finding_count,
        material_count,
    };
    store.save_bootstrap_report(identity.source.run_id, identity.phase, &stored)?;
    // Includes duplicate completion: refuse corruption/conflicts, never overwrite
    // an existing frozen report to hide a modification or missing evidence.
    reference.load(store, &identity)?;
    Ok(reference)
}

pub fn save_review(
    store: &RepositoryMemoryStore,
    identity: ReportIdentity,
    review: &CoverageReview,
) -> anyhow::Result<ReportReference> {
    ensure!(
        identity.phase == BootstrapReportKind::Review,
        "Review identity required"
    );
    let review = CoverageReview::parse(&serde_json::to_string(review)?)?;
    save(
        store,
        identity,
        serde_json::to_value(&review)?,
        Some(review.verdict),
        review.findings.len(),
        material_count(&review),
    )
}

pub fn save_refinement(
    store: &RepositoryMemoryStore,
    identity: ReportIdentity,
    report: &RefinementReport,
    review: &CoverageReview,
) -> anyhow::Result<ReportReference> {
    ensure!(
        identity.phase == BootstrapReportKind::Refine,
        "Refine identity required"
    );
    let report = RefinementReport::parse(&serde_json::to_string(report)?, review)?;
    save(
        store,
        identity,
        serde_json::to_value(&report)?,
        None,
        report.resolutions.len(),
        0,
    )
}

pub fn is_reference(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("report_version").cloned())
        .is_some()
}

/// The router consumes only data frozen by the host validator. File integrity is
/// checked again before Refine and publication (including the PASS path).
pub fn review_verdict(
    raw: &str,
    run_id: Uuid,
    workspace_id: Uuid,
) -> anyhow::Result<CoverageVerdict> {
    if !is_reference(raw) {
        return Ok(CoverageReview::parse(raw)?.verdict);
    }
    let reference = ReportReference::parse(raw)?;
    ensure!(
        reference.identity.source.run_id == run_id
            && reference.identity.source.workspace_id == workspace_id
            && reference.identity.phase == BootstrapReportKind::Review,
        "Foreign coverage routing reference"
    );
    reference.verdict.context("Missing coverage verdict")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn identity() -> ReportIdentity {
        ReportIdentity {
            source: InventoryIdentity::new(
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                "a".repeat(40),
            ),
            phase: BootstrapReportKind::Review,
            session_id: Uuid::new_v4(),
            agent_run_id: Uuid::new_v4(),
        }
    }

    fn review() -> CoverageReview {
        CoverageReview::parse(&json!({"version":1,"verdict":"needs_refinement","summary":"領域ごとの検証",
            "findings":(0..20).map(|n| json!({"severity":"material","title":format!("領域{n}"),
                "description":"不足する契約と根拠を記載。".repeat(400),"evidencePaths":["src/main.rs"],"recommendedAction":"expand_page"})).collect::<Vec<_>>()
        }).to_string()).unwrap()
    }

    #[test]
    fn file_handoff_preserves_reports_beyond_old_field_ceilings() {
        let dir = tempfile::tempdir().unwrap();
        let store = RepositoryMemoryStore::at_persistent(dir.path()).unwrap();
        let id = identity();
        let mut review = review();
        review.findings.truncate(1);
        review.summary = "検証概要".repeat(2100);
        review.findings[0].title = "重要な契約".repeat(40);
        review.findings[0].description = "根拠と指摘".repeat(2100);
        let prefix = "nested/".repeat(40);
        review.findings[0].evidence_paths = (0..40)
            .map(|i| format!("src/{prefix}evidence-{i}.rs"))
            .collect();
        let reference = save_review(&store, id.clone(), &review).unwrap();
        assert_eq!(reference.load_review(&store, &id).unwrap(), review);

        let raw = json!({"version": 1, "summary": "処置概要".repeat(2100), "resolutions": [{
            "findingIndex": 0, "disposition": "fixed", "reason": "検証と修正".repeat(2100),
            "evidencePaths": review.findings[0].evidence_paths,
            "wikiPaths": (0..40).map(|i| format!("openwiki/{prefix}concept-{i}.md")).collect::<Vec<_>>()
        }]});
        let report = RefinementReport::parse(&raw.to_string(), &review).unwrap();
        let mut id = id;
        id.phase = BootstrapReportKind::Refine;
        id.session_id = Uuid::new_v4();
        id.agent_run_id = Uuid::new_v4();
        let reference = save_refinement(&store, id.clone(), &report, &review).unwrap();
        let loaded = reference.load_refinement(&store, &id, &review).unwrap();
        assert_eq!(serde_json::to_value(loaded).unwrap(), raw);
    }

    #[test]
    fn full_reports_are_immutable_while_references_stay_small() {
        let dir = tempfile::tempdir().unwrap();
        let store = RepositoryMemoryStore::at_persistent(dir.path()).unwrap();
        let id = identity();
        let review = review();
        let reference = save_review(&store, id.clone(), &review).unwrap();
        assert!(
            std::fs::metadata(reference.prompt_path(&store))
                .unwrap()
                .len()
                > 128 * 1024
        );
        let raw = serde_json::to_string(&reference).unwrap();
        assert!(raw.len() < 4096);
        let parsed = ReportReference::parse(&raw).unwrap();
        assert_eq!(parsed.load_review(&store, &id).unwrap(), review);
        assert_eq!(
            review_verdict(&raw, id.source.run_id, id.source.workspace_id).unwrap(),
            CoverageVerdict::NeedsRefinement
        );
        assert_eq!(
            save_review(&store, id.clone(), &review).unwrap().digest,
            parsed.digest
        );
        let mut changed = review.clone();
        changed.summary.push('!');
        assert!(save_review(&store, id.clone(), &changed).is_err());
        let mut foreign = id.clone();
        foreign.source.source_sha = "b".repeat(40);
        assert!(parsed.load_review(&store, &foreign).is_err());
        foreign = id.clone();
        foreign.agent_run_id = Uuid::new_v4();
        assert!(parsed.load_review(&store, &foreign).is_err());
        assert!(review_verdict(&raw, Uuid::new_v4(), id.source.workspace_id).is_err());
        let mut incorrect_metadata = parsed.clone();
        incorrect_metadata.finding_count += 1;
        assert!(incorrect_metadata.load_review(&store, &id).is_err());
        std::fs::write(reference.prompt_path(&store), b"{}").unwrap();
        assert!(parsed.load_review(&store, &id).is_err());
        assert!(save_review(&store, id.clone(), &review).is_err());
        std::fs::remove_file(reference.prompt_path(&store)).unwrap();
        assert!(parsed.load_review(&store, &id).is_err());
    }

    #[test]
    fn refinement_file_preserves_all_resolutions_and_legacy_inline_verdict() {
        let dir = tempfile::tempdir().unwrap();
        let store = RepositoryMemoryStore::at_persistent(dir.path()).unwrap();
        let mut id = identity();
        id.phase = BootstrapReportKind::Refine;
        let review = review();
        let report = RefinementReport::parse(&json!({"version":1,"summary":"検証完了",
            "resolutions":(0..20).map(|n| json!({"findingIndex":n,"disposition":"refuted",
                "reason":"ソースと契約を検証。".repeat(100),"wikiPaths":[],"evidencePaths":["src/main.rs"]})).collect::<Vec<_>>()
        }).to_string(), &review).unwrap();
        let reference = save_refinement(&store, id.clone(), &report, &review).unwrap();
        let parsed = ReportReference::parse(&serde_json::to_string(&reference).unwrap()).unwrap();
        let loaded = parsed.load_refinement(&store, &id, &review).unwrap();
        assert_eq!(loaded.resolutions.len(), 20);
        assert!(!loaded.requires_update());
        assert!(
            review_verdict(
                &serde_json::to_string(&reference).unwrap(),
                id.source.run_id,
                id.source.workspace_id
            )
            .is_err()
        );
        assert_eq!(
            review_verdict(
                &serde_json::to_string(&review).unwrap(),
                id.source.run_id,
                id.source.workspace_id
            )
            .unwrap(),
            review.verdict
        );
    }

    #[cfg(unix)]
    #[test]
    fn report_io_rejects_symlinks_and_non_regular_files() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let store = RepositoryMemoryStore::at_persistent(dir.path()).unwrap();
        let id = identity();
        let reference = save_review(&store, id.clone(), &review()).unwrap();
        let path = reference.prompt_path(&store);
        let outside = dir.path().join("outside.json");
        std::fs::rename(&path, &outside).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(reference.load_review(&store, &id).is_err());
        assert!(save_review(&store, id.clone(), &review()).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(reference.load_review(&store, &id).is_err());
        std::fs::remove_dir(&path).unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(reference.load_review(&store, &id).is_err());
    }
}
