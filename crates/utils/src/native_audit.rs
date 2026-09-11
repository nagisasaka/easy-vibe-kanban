use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::assets::asset_dir;

pub const NATIVE_AUDIT_ROOT_RELATIVE: &str = "runtime/native-audit/v1";

pub fn session_dir(session_id: Uuid) -> PathBuf {
    session_dir_in_root(&asset_dir(), session_id)
}

pub fn session_dir_in_root(root: &Path, session_id: Uuid) -> PathBuf {
    let session = session_id.to_string();
    root.join(NATIVE_AUDIT_ROOT_RELATIVE)
        .join("sessions")
        .join(session.chars().take(2).collect::<String>())
        .join(session)
}

pub fn attempt_relative_dir(session_id: Uuid, agent_run_id: Uuid, run_attempt_id: Uuid) -> PathBuf {
    let session = session_id.to_string();
    PathBuf::from(NATIVE_AUDIT_ROOT_RELATIVE)
        .join("sessions")
        .join(session.chars().take(2).collect::<String>())
        .join(session)
        .join("agent-runs")
        .join(agent_run_id.to_string())
        .join("attempts")
        .join(run_attempt_id.to_string())
}
