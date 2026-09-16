//! Model-free test driver for scripts/test-openwiki-host.mjs --evk-setup.
//! Intentionally restricted to that script's disposable fixture directories.
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use services::services::openwiki::{publication_paths, setup};
use utils::repository_memory::RepositoryMemoryStore;
use uuid::Uuid;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 5,
        "Expected operation, fixture root, persistent directory, workspace UUID, source commit"
    );
    let root = PathBuf::from(&args[1]).canonicalize()?;
    let temp = std::env::temp_dir().canonicalize()?;
    ensure!(
        root.parent() == Some(temp.as_path())
            && root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("evk-openwiki-host-")),
        "This driver only operates on disposable OpenWiki smoke fixtures"
    );
    let persistent = Path::new(&args[2]).canonicalize()?;
    ensure!(
        persistent.parent() == Some(temp.as_path())
            && persistent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("evk-openwiki-memory-")),
        "Fixture memory must be a separate temporary directory"
    );
    let store = RepositoryMemoryStore::at_persistent(&persistent)?;
    let workspace_id = Uuid::parse_str(&args[3])?;
    let source = &args[4];
    ensure!(
        source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid source commit"
    );
    match args[0].as_str() {
        "prepare" => setup::prepare(&store, workspace_id, &root, source)?,
        "restore" => {
            setup::validate_provenance(&store, workspace_id, &root, source)?;
            setup::restore(&store, workspace_id, &root, source)?;
            setup::validate_provenance(&store, workspace_id, &root, source)?;
            let paths = publication_paths(&git::GitService::new(), &root, source)
                .context("Production Wiki publication guard")?;
            ensure!(
                paths.iter().all(|path| path.starts_with("openwiki/")),
                "Unexpected publication path"
            );
        }
        _ => bail!("Unsupported fixture operation"),
    }
    Ok(())
}
