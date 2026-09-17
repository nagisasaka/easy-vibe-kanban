//! OpenWiki proof joins all durable attempts, not just available/latest audits.
use std::path::Path;

use anyhow::{Context, ensure};
use db::models::agent_runtime::{
    AgentProcessRegistryRecord, AgentRunAttemptRecord, AgentRunRecord, NativeAuditStreamRecord,
};
use executors::runtime::{AgentRunStatus, NativeAuditReader};
use services::services::openwiki::completion::{PhaseCompletionProof, WriterPhase};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Terminal projection commits just before process-exit registration. The
/// existing monitor may poll again during this bounded normal handoff.
#[derive(Debug, thiserror::Error)]
#[error("Waiting for OpenWiki process-exit registration")]
pub(crate) struct CompletionPending;

#[derive(Debug, thiserror::Error)]
#[error("OpenWiki cleanup is unconfirmed; maintenance remains fenced: {0}")]
pub(crate) struct MaintenanceFenced(pub String);

/// Terminal projection is not evidence of exit. This also protects restoration
/// after cancellation/failure, when there is no successful operation proof.
pub(crate) async fn ensure_processes_exited(
    pool: &SqlitePool,
    run_id: Uuid,
    status: AgentRunStatus,
    updated_at: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<()> {
    let attempts: Vec<(Uuid, AgentRunStatus)> = sqlx::query_as(
        "SELECT id, status FROM agent_run_attempts WHERE agent_run_id = ? ORDER BY attempt_number",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    if attempts.is_empty() || !status.is_terminal() {
        return Err(MaintenanceFenced("Missing attempts or active AgentRun".into()).into());
    }
    for (index, (attempt_id, attempt_status)) in attempts.iter().enumerate() {
        let process: Option<AgentProcessRegistryRecord> =
            sqlx::query_as("SELECT * FROM agent_process_registry WHERE run_attempt_id = ?")
                .bind(attempt_id)
                .fetch_optional(pool)
                .await?;
        if let Some(process) = &process {
            // A host endpoint is durably reserved before spawning. No endpoint
            // on a failed terminal attempt means no spawn, or confirmed startup
            // cleanup. This is cleanup evidence only, never successful proof.
            if attempt_status.is_terminal()
                && *attempt_status != AgentRunStatus::Succeeded
                && process.registry_status == "reserved"
                && process.host_endpoint.is_none()
                && process.host_token.is_none()
                && process.host_pid.is_none()
                && process.pid.is_none()
            {
                continue;
            }
            if attempt_status.is_terminal()
                && process.registry_status == "exited"
                && process.observed_exited_at.is_some()
            {
                continue;
            }
            if index + 1 == attempts.len()
                && matches!(process.registry_status.as_str(), "spawned" | "running")
                && chrono::Utc::now()
                    .signed_duration_since(updated_at)
                    .num_seconds()
                    < 10
            {
                return Err(CompletionPending.into());
            }
        }
        // A reserved endpoint may already have a live host: never infer exit
        // from provider PID absence alone.
        return Err(MaintenanceFenced(format!(
            "Attempt {} process exit is unconfirmed",
            attempt_id
        ))
        .into());
    }
    Ok(())
}

pub(crate) async fn writer_completion_proof(
    pool: &SqlitePool,
    run: &AgentRunRecord,
    workspace_id: Uuid,
    workspace_path: &Path,
    root: &Path,
    phase: WriterPhase,
) -> anyhow::Result<PhaseCompletionProof> {
    ensure!(
        run.workspace_id == workspace_id && run.status == AgentRunStatus::Succeeded,
        "OpenWiki AgentRun is not a successful delegated workspace run"
    );
    let attempts: Vec<AgentRunAttemptRecord> = sqlx::query_as(
        "SELECT * FROM agent_run_attempts WHERE agent_run_id = ? ORDER BY attempt_number",
    )
    .bind(run.id)
    .fetch_all(pool)
    .await?;
    ensure!(!attempts.is_empty(), "OpenWiki has no RunAttempts");
    ensure!(
        attempts.last().unwrap().status == AgentRunStatus::Succeeded,
        "Latest OpenWiki attempt did not succeed"
    );
    let mut proof = PhaseCompletionProof::new(phase);
    let mut previous_exit = None;
    for (index, attempt) in attempts.iter().enumerate() {
        let request = &attempt.request_envelope.0;
        request.validate_for_run(&run.request_envelope.0)?;
        ensure!(
            attempt.attempt_number == index as i64 + 1
                && request.run_attempt_id == attempt.id
                && request.agent_run_id == run.id
                && request.session_id == run.session_id
                && request.turn_id == attempt.turn_id
                && i64::from(request.attempt_number) == attempt.attempt_number
                && request.workspace.workspace_id == workspace_id,
            "OpenWiki attempt identity/sequence mismatch"
        );
        ensure!(
            attempt.status.is_terminal(),
            "Earlier OpenWiki attempt is still active"
        );
        let process: AgentProcessRegistryRecord =
            sqlx::query_as("SELECT * FROM agent_process_registry WHERE run_attempt_id = ?")
                .bind(attempt.id)
                .fetch_one(pool)
                .await?;
        if index + 1 == attempts.len()
            && matches!(process.registry_status.as_str(), "spawned" | "running")
            && process.observed_exited_at.is_none()
            && chrono::Utc::now()
                .signed_duration_since(run.updated_at)
                .num_seconds()
                < 10
        {
            return Err(CompletionPending.into());
        }
        ensure!(
            process.registry_status == "exited" && process.observed_exited_at.is_some(),
            "OpenWiki attempt {} process exit is unconfirmed",
            attempt.id
        );
        if let (Some(previous), Some(started)) = (previous_exit, process.process_started_at) {
            ensure!(previous <= started, "OpenWiki attempts overlapped");
        }
        previous_exit = process.observed_exited_at;
        let stream: NativeAuditStreamRecord =
            sqlx::query_as("SELECT * FROM native_audit_streams WHERE run_attempt_id = ?")
                .bind(attempt.id)
                .fetch_optional(pool)
                .await?
                .with_context(|| {
                    format!(
                        "OpenWiki attempt {} audit missing; side effects cannot be excluded",
                        attempt.id
                    )
                })?;
        let path = utils::assets::asset_dir().join(&stream.manifest_relative_path);
        let audit = NativeAuditReader::read(path.parent().context("Invalid audit path")?)
            .with_context(|| format!("OpenWiki attempt {} audit integrity failed", attempt.id))?;
        let manifest = audit.manifest();
        ensure!(
            stream.session_id == run.session_id
                && stream.agent_run_id == run.id
                && manifest.session_id == run.session_id
                && manifest.agent_run_id == run.id
                && manifest.run_attempt_id == attempt.id
                && manifest.turn_id == attempt.turn_id
                && i64::from(manifest.run_attempt_number) == attempt.attempt_number
                && manifest.provider_id == "codex"
                && request.provider_id == "codex"
                && Path::new(&request.workspace.path).canonicalize()?
                    == workspace_path.canonicalize()?
                && Path::new(&manifest.workspace_path).canonicalize()?
                    == workspace_path.canonicalize()?,
            "OpenWiki Native Audit identity does not match the delegated attempt"
        );
        let root_observed = proof
            .replay_attempt(&audit, root)
            .with_context(|| format!("OpenWiki attempt {} operation proof failed", attempt.id))?;
        ensure!(
            attempt.status != AgentRunStatus::Succeeded || root_observed,
            "Successful OpenWiki attempt has no audited root thread"
        );
    }
    proof.validate()?;
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use executors::{executors::BaseCodingAgent, profile::ExecutorConfig, runtime::*};
    use serde_json::{Value, json};

    use super::*;

    struct Fixture {
        temp: tempfile::TempDir,
        pool: SqlitePool,
        request: AgentRunRequestEnvelope,
        attempts: Vec<RunAttemptRequest>,
    }

    impl Fixture {
        async fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
            let workspace_id = Uuid::new_v4();
            let session_id = Uuid::new_v4();
            sqlx::query("INSERT INTO workspaces (id, branch) VALUES (?, 'fixture')")
                .bind(workspace_id)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO sessions (id, workspace_id) VALUES (?, ?)")
                .bind(session_id)
                .bind(workspace_id)
                .execute(&pool)
                .await
                .unwrap();
            let now = chrono::Utc::now();
            let request = AgentRunRequestEnvelope {
                schema_version: AGENT_REQUEST_SCHEMA_VERSION,
                payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
                request_id: Uuid::new_v4(),
                idempotency_key: Uuid::new_v4().to_string(),
                session_id,
                agent_run_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                correlation_id: Uuid::new_v4(),
                intent: AgentRunIntent::Initial,
                runtime_profile_id: ExecutorConfig::new(BaseCodingAgent::Codex)
                    .profile_id()
                    .cache_key(),
                provider_id: "codex".into(),
                workspace: WorkspaceReference {
                    workspace_id,
                    mode: WorkspaceMode::IsolatedWorktree,
                    path: temp.path().to_string_lossy().into_owned(),
                },
                input: CanonicalMessage {
                    message_id: Uuid::new_v4(),
                    role: AgentRuntimeMessageRole::User,
                    content: "OpenWiki fixture".into(),
                },
                created_at: now,
            };
            Self {
                temp,
                pool,
                request,
                attempts: vec![],
            }
        }

        async fn attempt(&mut self, frames: Option<Vec<Value>>) {
            let request = &self.request;
            let now = chrono::Utc::now();
            let attempt = RunAttemptRequest {
                schema_version: AGENT_REQUEST_SCHEMA_VERSION,
                payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
                request_id: Uuid::new_v4(),
                idempotency_key: Uuid::new_v4().to_string(),
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                turn_id: request.turn_id,
                run_attempt_id: Uuid::new_v4(),
                attempt_number: self.attempts.len() as u32 + 1,
                correlation_id: request.correlation_id,
                mode: RunAttemptMode::Launch,
                transport: AgentTransportKind::AppServerJsonrpc,
                runtime_profile_id: request.runtime_profile_id.clone(),
                provider_id: "codex".into(),
                workspace: request.workspace.clone(),
                capability_snapshot: CapabilitySnapshot {
                    schema_version: 1,
                    runtime_profile_id: request.runtime_profile_id.clone(),
                    provider_id: "codex".into(),
                    runtime_version: None,
                    protocol_version: None,
                    adapter_version: "fixture".into(),
                    resolved_at: now,
                    capabilities: vec![],
                },
                executor_config: ExecutorConfig::new(BaseCodingAgent::Codex),
                selected_skills: None,
                reset_to_message_id: None,
                provider_session: None,
                created_at: now,
            };
            if self.attempts.is_empty() {
                AgentRunRecord::persist_identity_before_launch(&self.pool, request, &attempt)
                    .await
                    .unwrap();
            } else {
                AgentRunRecord::persist_retry_attempt_before_launch(&self.pool, request, &attempt)
                    .await
                    .unwrap();
                sqlx::query("UPDATE agent_run_attempts SET status = 'failed' WHERE agent_run_id = ? AND id != ?")
                    .bind(request.agent_run_id).bind(attempt.run_attempt_id).execute(&self.pool).await.unwrap();
            }
            sqlx::query(
                "UPDATE agent_run_attempts SET status = 'succeeded', finished_at = ? WHERE id = ?",
            )
            .bind(now)
            .bind(attempt.run_attempt_id)
            .execute(&self.pool)
            .await
            .unwrap();
            sqlx::query("UPDATE agent_runs SET status = 'succeeded' WHERE id = ?")
                .bind(request.agent_run_id)
                .execute(&self.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE agent_process_registry SET registry_status = 'exited', pid = 123, process_started_at = ?, observed_exited_at = ? WHERE run_attempt_id = ?")
                .bind(now).bind(now).bind(attempt.run_attempt_id).execute(&self.pool).await.unwrap();
            if let Some(frames) = frames {
                let mut writer = NativeAuditWriter::create_in(
                    self.temp.path(),
                    NativeAuditMetadata {
                        session_id: request.session_id,
                        agent_run_id: request.agent_run_id,
                        turn_id: request.turn_id,
                        run_attempt_id: attempt.run_attempt_id,
                        run_attempt_number: attempt.attempt_number,
                        provider_id: "codex".into(),
                        runtime_profile_id: request.runtime_profile_id.clone(),
                        workspace_path: request.workspace.path.clone(),
                        runtime_version: None,
                        protocol_version: None,
                        adapter_version: "fixture".into(),
                        mapper_version: "fixture".into(),
                        created_at: now,
                    },
                )
                .unwrap();
                let header =
                    json!({"id":3,"result":{"thread":{"id":"root"},"cwd":self.temp.path()}});
                for frame in std::iter::once(header).chain(frames) {
                    writer
                        .append_bytes(
                            NativeAuditDirection::Output,
                            NativeAuditChannel::Stdout,
                            "application/json",
                            Uuid::new_v4(),
                            &serde_json::to_vec(&frame).unwrap(),
                            None,
                        )
                        .unwrap();
                }
                let manifest = writer.close().unwrap();
                // Absolute test path keeps fixtures out of the user's asset tree.
                sqlx::query("INSERT INTO native_audit_streams (id, session_id, agent_run_id, run_attempt_id, audit_schema_version, adapter_version, mapper_version, manifest_relative_path, frames_relative_path, integrity_status) VALUES (?, ?, ?, ?, 1, 'fixture', 'fixture', ?, ?, 'complete')")
                    .bind(attempt.run_attempt_id).bind(request.session_id).bind(request.agent_run_id).bind(attempt.run_attempt_id)
                    .bind(self.temp.path().join(manifest.manifest_relative_path).to_str())
                    .bind(self.temp.path().join(manifest.frames_relative_path).to_str()).execute(&self.pool).await.unwrap();
            }
            self.attempts.push(attempt);
        }

        async fn proof(&self, phase: WriterPhase) -> anyhow::Result<PhaseCompletionProof> {
            let run = AgentRunRecord::find(&self.pool, self.request.agent_run_id)
                .await?
                .unwrap();
            writer_completion_proof(
                &self.pool,
                &run,
                self.request.workspace.workspace_id,
                self.temp.path(),
                self.temp.path(),
                phase,
            )
            .await
        }

        fn operations(&self, modes: &[&str], finish_last: bool) -> Vec<Value> {
            let mut frames = vec![];
            for (index, mode) in modes.iter().enumerate() {
                let run = Uuid::new_v4();
                for (tool, args, result) in [
                    (
                        "openwiki_begin",
                        json!({"root":self.temp.path(),"mode":mode,"force":true}),
                        json!({"status":"active","root":self.temp.path(),"mode":mode,"runId":run,"resumed":false}),
                    ),
                    (
                        "openwiki_finish",
                        json!({"runId":run}),
                        json!({"status":"complete"}),
                    ),
                ] {
                    if tool == "openwiki_finish" && index == modes.len() - 1 && !finish_last {
                        continue;
                    }
                    let id = Uuid::new_v4();
                    for method in ["item/started", "item/completed"] {
                        frames.push(json!({"method":method,"params":{"threadId":"root","item":{
                            "id":id,"type":"mcpToolCall","server":"openwiki","tool":tool,"arguments":args,
                            "result":{"structuredContent":result},"error":null,"status":"completed"
                        }}}));
                    }
                }
            }
            frames
        }
    }

    #[tokio::test]
    async fn bootstrap_production_gate_accepts_generate_self_correction_and_optional_refine() {
        let mut fixture = Fixture::new().await;
        fixture
            .attempt(Some(
                fixture.operations(&["init", "update", "update"], true),
            ))
            .await;
        fixture.proof(WriterPhase::Generate).await.unwrap();
        let mut fixture = Fixture::new().await;
        fixture.attempt(Some(vec![])).await;
        assert!(
            !fixture
                .proof(WriterPhase::Refine)
                .await
                .unwrap()
                .has_completed_update()
        );
        assert!(fixture.proof(WriterPhase::Generate).await.is_err());
    }

    #[tokio::test]
    async fn bootstrap_exit_registration_race_is_pending_but_not_indefinite() {
        let mut f = Fixture::new().await;
        f.attempt(Some(f.operations(&["init"], true))).await;
        sqlx::query("UPDATE agent_process_registry SET registry_status = 'running', observed_exited_at = NULL")
            .execute(&f.pool).await.unwrap();
        let error = f.proof(WriterPhase::Generate).await.unwrap_err();
        assert!(error.downcast_ref::<CompletionPending>().is_some());
        sqlx::query("UPDATE agent_runs SET updated_at = ?")
            .bind(chrono::Utc::now() - chrono::Duration::seconds(20))
            .execute(&f.pool)
            .await
            .unwrap();
        let error = f.proof(WriterPhase::Generate).await.unwrap_err();
        assert!(error.downcast_ref::<CompletionPending>().is_none());
        sqlx::query(
            "UPDATE agent_process_registry SET registry_status = 'exited', observed_exited_at = ?",
        )
        .bind(chrono::Utc::now())
        .execute(&f.pool)
        .await
        .unwrap();
        f.proof(WriterPhase::Generate).await.unwrap();
    }

    #[tokio::test]
    async fn bootstrap_production_gate_checks_all_attempts_not_latest_audit() {
        for earlier in [
            "empty",
            "missing",
            "unfinished",
            "bad_identity",
            "still_running",
            "corrupt",
        ] {
            let mut f = Fixture::new().await;
            let frames = match earlier {
                "missing" => None,
                "unfinished" => Some(f.operations(&["init"], false)),
                _ => Some(vec![]),
            };
            f.attempt(frames).await;
            f.attempt(Some(f.operations(&["init"], true))).await;
            let first = f.attempts[0].run_attempt_id;
            match earlier {
                "bad_identity" => {
                    let path: String = sqlx::query_scalar("SELECT manifest_relative_path FROM native_audit_streams WHERE run_attempt_id = ?")
                        .bind(first).fetch_one(&f.pool).await.unwrap();
                    let mut manifest: Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    manifest["session_id"] = json!(Uuid::new_v4());
                    std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
                }
                "still_running" => {
                    sqlx::query("UPDATE agent_process_registry SET registry_status = 'unreachable', observed_exited_at = NULL WHERE run_attempt_id = ?")
                        .bind(first).execute(&f.pool).await.unwrap();
                }
                "corrupt" => {
                    let path: String = sqlx::query_scalar("SELECT frames_relative_path FROM native_audit_streams WHERE run_attempt_id = ?")
                        .bind(first).fetch_one(&f.pool).await.unwrap();
                    std::fs::write(path, "corrupt\n").unwrap();
                }
                _ => {}
            }
            let result = f.proof(WriterPhase::Generate).await;
            assert_eq!(result.is_ok(), earlier == "empty", "{earlier}: {result:?}");
        }
    }

    #[tokio::test]
    async fn sync_gate_checks_all_attempts_and_process_exit_before_cleanup() {
        for earlier in ["empty", "missing", "unfinished"] {
            let mut f = Fixture::new().await;
            f.attempt(match earlier {
                "missing" => None,
                "unfinished" => Some(f.operations(&["update"], false)),
                _ => Some(vec![]),
            })
            .await;
            f.attempt(Some(f.operations(&["update", "update"], true)))
                .await;
            assert_eq!(
                f.proof(WriterPhase::Sync).await.is_ok(),
                earlier == "empty",
                "{earlier}"
            );
        }
        let mut f = Fixture::new().await;
        f.attempt(Some(vec![])).await;
        assert!(f.proof(WriterPhase::Sync).await.is_err());
        let mut run = AgentRunRecord::find(&f.pool, f.request.agent_run_id)
            .await
            .unwrap()
            .unwrap();
        run.status = AgentRunStatus::Cancelled;
        ensure_processes_exited(&f.pool, run.id, run.status, run.updated_at)
            .await
            .unwrap();
        sqlx::query("UPDATE agent_process_registry SET registry_status = 'running', observed_exited_at = NULL").execute(&f.pool).await.unwrap();
        assert!(
            ensure_processes_exited(&f.pool, run.id, run.status, run.updated_at)
                .await
                .unwrap_err()
                .downcast_ref::<CompletionPending>()
                .is_some()
        );
        run.updated_at -= chrono::Duration::seconds(20);
        assert!(
            ensure_processes_exited(&f.pool, run.id, run.status, run.updated_at)
                .await
                .unwrap_err()
                .downcast_ref::<MaintenanceFenced>()
                .is_some()
        );
    }
}
