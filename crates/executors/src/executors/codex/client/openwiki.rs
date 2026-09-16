//! Preflight the pinned OpenWiki host tools before a maintenance writer starts.
//! This is capability discovery only: it never begins or modifies an OpenWiki run.

use std::{collections::HashSet, io, time::Duration};

use super::AppServerClient;
use crate::executors::ExecutorError;

pub(super) const REQUIRED_TOOLS: [&str; 6] = [
    "openwiki_begin",
    "openwiki_submit_plan",
    "openwiki_next_page",
    "openwiki_inspect_page_claims",
    "openwiki_submit_page",
    "openwiki_finish",
];

// Separate from model/tool execution timeouts. Allow the 10-second MCP startup
// plus a small RPC margin, but never hang startup on an unresponsive catalog.
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(15);

#[cfg(test)]
mod tests;

impl AppServerClient {
    pub(crate) async fn ensure_openwiki_tools(&self) -> Result<(), ExecutorError> {
        self.ensure_openwiki_tools_with_timeout(PREFLIGHT_TIMEOUT)
            .await
    }

    pub(super) async fn ensure_openwiki_tools_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), ExecutorError> {
        let discover = async {
            if self.thread_id.lock().await.is_none() {
                return Err(io::Error::other("no registered Codex thread").into());
            }
            let mut cursor = None;
            let mut seen = HashSet::new();
            for _ in 0..64 {
                let response = self.list_mcp_server_status(cursor).await?;
                if let Some(server) = response
                    .data
                    .iter()
                    .find(|server| server.name == "openwiki")
                {
                    let missing: Vec<_> = REQUIRED_TOOLS
                        .iter()
                        .filter(|name| {
                            server
                                .tools
                                .get(**name)
                                .is_none_or(|tool| tool.name != **name)
                        })
                        .copied()
                        .collect();
                    if !missing.is_empty() {
                        return Err(io::Error::other(format!(
                            "registered server is missing required tools: {}. Check OpenWiki installation and MCP tool allow/deny settings",
                            missing.join(", ")
                        )).into());
                    }
                    return Ok(());
                }
                cursor = response.next_cursor;
                let Some(next) = &cursor else {
                    return Err(
                        io::Error::other("registered OpenWiki server is unavailable").into(),
                    );
                };
                if !seen.insert(next.clone()) {
                    return Err(io::Error::other("MCP catalog repeated a pagination cursor").into());
                }
            }
            Err::<(), ExecutorError>(
                io::Error::other("MCP catalog exceeded the pagination safety limit").into(),
            )
        };
        match tokio::time::timeout(timeout, discover).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(ExecutorError::Io(io::Error::other(format!(
                "OpenWiki MCP preflight failed; writer was not started: {error}"
            )))),
            Err(_) => Err(ExecutorError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "OpenWiki MCP preflight timed out; writer was not started",
            ))),
        }
    }
}
