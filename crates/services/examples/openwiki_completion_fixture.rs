//! Read-only protocol diagnostic; DB ownership/publication gates are not run.
use anyhow::{Context, ensure};
use executors::runtime::NativeAuditReader;
use services::services::openwiki::completion::{PhaseCompletionProof, WriterPhase};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    ensure!(
        args.len() >= 4,
        "usage: openwiki_completion_fixture generate|refine|sync REPOSITORY_ROOT AUDIT_ATTEMPT_DIR..."
    );
    let phase = match args[1].as_str() {
        "generate" => WriterPhase::Generate,
        "refine" => WriterPhase::Refine,
        "sync" => WriterPhase::Sync,
        _ => anyhow::bail!("phase must be generate, refine or sync"),
    };
    let mut proof = PhaseCompletionProof::new(phase);
    for directory in &args[3..] {
        let audit = NativeAuditReader::read(directory).context("Native Audit integrity")?;
        proof.replay_attempt(&audit, std::path::Path::new(&args[2]))?;
    }
    proof.validate()?;
    println!(
        "Protocol sequence verified (not publication approval); completed update: {}",
        proof.has_completed_update()
    );
    Ok(())
}
