use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use command_group::AsyncGroupChild;
use db::{
    DBService,
    models::{
        agent_runtime::{AgentEventRecord, AgentRunRecord, NativeAuditStreamRecord},
        session::Session,
        workspace::Workspace,
        workspace_repo::WorkspaceRepo,
    },
};
use executors::{
    actions::SelectedSkill,
    env::{ExecutionEnv, RepoContext},
    executors::provider_adapter::{
        DirectControl, DirectIntent, DirectProvider, encode_control, require_capability,
    },
    profile::ExecutorConfig,
    runtime::{
        AGENT_EVENT_PAYLOAD_VERSION, AGENT_EVENT_SCHEMA_VERSION, AgentCapability,
        AgentEventEnvelope, AgentEventPayload, AgentEventStream, AgentLiveEvent, AgentRunIntent,
        AgentRunPort, AgentRunPortCommand, AgentRunPortCommandEnvelope, AgentRunPortError,
        AgentRunPortSnapshot, AgentRunRequestEnvelope, AgentRunStatus, AgentRuntimeError,
        AgentRuntimeErrorKind, AgentRuntimeMessageRole, CanonicalMessage, NativeAuditMetadata,
        NativeAuditReference, NativeAuditWriter, ProjectionStatus, ProviderSessionReference,
        RunAttemptMode, RunAttemptRequest,
    },
};
use futures::{StreamExt, stream};
use rand::{RngCore, rngs::OsRng};
use serde::Serialize;
use sqlx::types::Json;
use tokio::{
    process::Command,
    sync::{Mutex, RwLock, broadcast},
};
use uuid::Uuid;

use crate::{
    agent_process_registry::{
        AgentProcessRegistry, RegisteredAgentProcess, RegisteredProcessPresence,
    },
    process_host::{
        HOST_PROTOCOL_VERSION, HostBootstrap, HostCommand, HostEventPayload, HostExecutionEnv,
        HostLaunchRequest, HostReady, journal::HostJournalReplay, read_host_subscription,
        send_host_command, subscribe_host_events,
    },
    transport::{read_json_frame, write_json_frame},
};

type SharedChild = Arc<RwLock<AsyncGroupChild>>;
const TERMINAL_EVENT_CHANNEL_CAPACITY: usize = 256;
const CANCEL_HOST_REPLAY_DEADLINE: Duration = Duration::from_secs(5);
const CANCEL_HOST_REPLAY_INTERVAL: Duration = Duration::from_millis(250);
const CANCELLING_OBSERVER_FAILURE_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancellationCleanupPreparation {
    OwnedProcessRegistered,
    ProcessAlreadyExited,
    ProcessAbsenceUnconfirmed,
}

type PersistedCancellationProcess = (String, Option<i64>, Option<i64>, Option<String>);

#[derive(sqlx::FromRow)]
struct HostObservation {
    host_endpoint: Option<String>,
    host_token: Option<String>,
    host_instance_id: Option<String>,
    last_host_event_sequence: i64,
    registry_status: String,
    host_pid: Option<i64>,
    pid: Option<i64>,
    process_group_id: Option<i64>,
    host_protocol_version: Option<i64>,
    host_start_identity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AgentRunTerminalEvent {
    pub agent_run_id: Uuid,
    pub session_id: Uuid,
    pub status: AgentRunStatus,
}

#[derive(Clone)]
pub struct LocalAgentRunPort {
    db: DBService,
    process_registry: AgentProcessRegistry,
    children: Arc<RwLock<HashMap<Uuid, SharedChild>>>,
    launching_attempts: Arc<Mutex<HashSet<Uuid>>>,
    observing_attempts: Arc<Mutex<HashSet<Uuid>>>,
    audit_writers: Arc<Mutex<HashMap<Uuid, NativeAuditWriter>>>,
    event_senders: Arc<RwLock<HashMap<Uuid, broadcast::Sender<AgentEventEnvelope>>>>,
    live_event_senders: Arc<RwLock<HashMap<Uuid, broadcast::Sender<AgentLiveEvent>>>>,
    terminal_event_sender: broadcast::Sender<AgentRunTerminalEvent>,
    event_write_lock: Arc<Mutex<()>>,
    command_lock: Arc<Mutex<()>>,
    cancellation_reconciliation_locks: Arc<Mutex<HashMap<Uuid, Weak<Mutex<()>>>>>,
}

struct FrozenDirectProviderLaunchSpec {
    provider: DirectProvider,
    executor_config: ExecutorConfig,
    intent: DirectIntent,
    prompt: String,
    provider_session: Option<ProviderSessionReference>,
    reset_to_message_id: Option<String>,
    selected_skills: Vec<SelectedSkill>,
    current_dir: PathBuf,
    env: ExecutionEnv,
}

#[derive(Serialize)]
struct AuditedDirectProviderLaunchSpec<'a> {
    schema_version: u16,
    provider: DirectProvider,
    executor_config: &'a ExecutorConfig,
    intent: DirectIntent,
    prompt: &'a str,
    provider_session: Option<&'a ProviderSessionReference>,
    reset_to_message_id: Option<&'a str>,
    selected_skills: &'a [SelectedSkill],
    approval_behavior: &'static str,
    current_dir: &'a Path,
    env: AuditedExecutionEnv<'a>,
}

#[derive(Serialize)]
struct AuditedExecutionEnv<'a> {
    vars: &'a HashMap<String, String>,
    workspace_root: &'a Path,
    repo_names: &'a [String],
    commit_reminder: bool,
    commit_reminder_prompt: &'a str,
}

impl FrozenDirectProviderLaunchSpec {
    fn new(
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        provider: DirectProvider,
        env: ExecutionEnv,
    ) -> Result<Self, AgentRunPortError> {
        let prompt = executors::legacy_wiki::execution_prompt(&request.input.content)
            .map_err(AgentRunPortError::Rejected)?;
        Ok(Self {
            provider,
            executor_config: attempt.executor_config.clone(),
            intent: direct_intent(request.intent, attempt.mode),
            prompt: executors::executors::provider_adapter::prompt_with_repository_memory(
                provider, &prompt, &env,
            ),
            provider_session: attempt.provider_session.clone(),
            reset_to_message_id: attempt.reset_to_message_id.clone(),
            selected_skills: executors::legacy_wiki::filter_skills(
                attempt.selected_skills.clone().unwrap_or_default(),
            ),
            current_dir: PathBuf::from(&attempt.workspace.path),
            env,
        })
    }

    #[cfg(test)]
    fn launch_request(
        &self,
    ) -> executors::executors::provider_adapter::DirectProviderLaunchRequest<'_> {
        executors::executors::provider_adapter::DirectProviderLaunchRequest {
            provider: self.provider,
            executor_config: &self.executor_config,
            intent: self.intent,
            prompt: &self.prompt,
            provider_session: self.provider_session.as_ref(),
            reset_to_message_id: self.reset_to_message_id.as_deref(),
            selected_skills: &self.selected_skills,
            approvals: Arc::new(executors::approvals::NoopExecutorApprovalService),
            current_dir: &self.current_dir,
            env: &self.env,
        }
    }

    fn audit_payload(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&AuditedDirectProviderLaunchSpec {
            schema_version: 1,
            provider: self.provider,
            executor_config: &self.executor_config,
            intent: self.intent,
            prompt: &self.prompt,
            provider_session: self.provider_session.as_ref(),
            reset_to_message_id: self.reset_to_message_id.as_deref(),
            selected_skills: &self.selected_skills,
            approval_behavior: "noop",
            current_dir: &self.current_dir,
            env: AuditedExecutionEnv {
                vars: &self.env.vars,
                workspace_root: &self.env.repo_context.workspace_root,
                repo_names: &self.env.repo_context.repo_names,
                commit_reminder: self.env.commit_reminder,
                commit_reminder_prompt: &self.env.commit_reminder_prompt,
            },
        })
    }
}

fn direct_intent(intent: AgentRunIntent, mode: RunAttemptMode) -> DirectIntent {
    match (intent, mode) {
        (_, RunAttemptMode::Resume) => DirectIntent::Resume,
        (AgentRunIntent::Initial, RunAttemptMode::Launch | RunAttemptMode::Restart) => {
            DirectIntent::Initial
        }
        (AgentRunIntent::FollowUp, RunAttemptMode::Launch) => DirectIntent::FollowUp,
        (AgentRunIntent::FollowUp, RunAttemptMode::Restart) => DirectIntent::Initial,
        (AgentRunIntent::Review, RunAttemptMode::Launch | RunAttemptMode::Restart) => {
            DirectIntent::Review
        }
    }
}

impl LocalAgentRunPort {
    pub(crate) fn new(db: DBService) -> Self {
        let (terminal_event_sender, _) = broadcast::channel(TERMINAL_EVENT_CHANNEL_CAPACITY);
        Self {
            db,
            process_registry: AgentProcessRegistry::default(),
            children: Arc::new(RwLock::new(HashMap::new())),
            launching_attempts: Arc::new(Mutex::new(HashSet::new())),
            observing_attempts: Arc::new(Mutex::new(HashSet::new())),
            audit_writers: Arc::new(Mutex::new(HashMap::new())),
            event_senders: Arc::new(RwLock::new(HashMap::new())),
            live_event_senders: Arc::new(RwLock::new(HashMap::new())),
            terminal_event_sender,
            event_write_lock: Arc::new(Mutex::new(())),
            command_lock: Arc::new(Mutex::new(())),
            cancellation_reconciliation_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn subscribe_terminal_events(&self) -> broadcast::Receiver<AgentRunTerminalEvent> {
        self.terminal_event_sender.subscribe()
    }

    pub async fn subscribe_live_events(
        &self,
        agent_run_id: Uuid,
    ) -> broadcast::Receiver<AgentLiveEvent> {
        self.live_sender(agent_run_id).await.subscribe()
    }

    /// Reconnect to every persisted durable process host during application startup.
    ///
    /// This is intentionally public so server startup can invoke reconciliation
    /// before accepting AgentRun traffic. Reconciliation only attaches to the
    /// endpoint/token persisted for each RunAttempt; it never replacement-spawns
    /// a provider when the host is unreachable.
    pub async fn reconcile_process_hosts(&self) {
        let run_attempts: Vec<(Uuid, Uuid)> = match sqlx::query_as(
            r#"
            SELECT apr.run_attempt_id, ara.agent_run_id
            FROM agent_process_registry apr
            JOIN agent_run_attempts ara ON ara.id = apr.run_attempt_id
            WHERE apr.registry_status IN ('reserved', 'spawned', 'running', 'unreachable')
              AND apr.host_endpoint IS NOT NULL AND apr.host_token IS NOT NULL
            "#,
        )
        .fetch_all(&self.db.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "failed to load process-host attachments for reconciliation");
                return;
            }
        };
        for (run_attempt_id, agent_run_id) in run_attempts {
            if let Err(error) = self.attach_process_host(run_attempt_id).await {
                if let Err(mark_error) =
                    AgentRunRecord::mark_process_host_unreachable(&self.db.pool, run_attempt_id)
                        .await
                {
                    tracing::error!(run_attempt_id = %run_attempt_id, %mark_error, "failed to preserve unreachable process host");
                }
                tracing::warn!(run_attempt_id = %run_attempt_id, %error, "process host unavailable during startup reconciliation");
                if let Ok((request, attempt)) = self.load_request(agent_run_id).await {
                    let port = self.clone();
                    tokio::spawn(async move {
                        port.observe_process_host(request, attempt).await;
                    });
                }
            }
        }
    }

    /// Persist a complete AgentRun identity without launching its provider.
    ///
    /// Product-level launch gates use this when an independent setup script
    /// must finish before the Agent process is allowed to start.
    pub async fn reserve(
        &self,
        request: AgentRunRequestEnvelope,
        attempt: RunAttemptRequest,
    ) -> Result<Uuid, AgentRunPortError> {
        attempt
            .validate_for_run(&request)
            .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
        self.validate_workspace(&request).await?;
        AgentRunRecord::persist_identity_before_launch(&self.db.pool, &request, &attempt)
            .await
            .map(|identity| identity.agent_run_id)
            .map_err(|error| AgentRunPortError::Rejected(error.to_string()))
    }

    /// Launch a previously reserved AgentRun exactly once per local runtime.
    /// Repeated calls after launch acceptance return the current snapshot.
    pub async fn launch_reserved(
        &self,
        agent_run_id: Uuid,
    ) -> Result<AgentRunPortSnapshot, AgentRunPortError> {
        let _command_guard = self.command_lock.lock().await;
        let snapshot = self.query(agent_run_id).await?;
        if snapshot.state.status != AgentRunStatus::Pending {
            return Ok(snapshot);
        }

        let (request, attempt) = self.load_request(agent_run_id).await?;
        let workspace = self.validate_workspace(&request).await?;
        self.start_attempt(request, attempt, workspace).await;
        self.query(agent_run_id).await
    }

    /// Terminalize a setup-gated AgentRun that was reserved but never launched.
    pub async fn fail_reserved(
        &self,
        agent_run_id: Uuid,
        message: String,
    ) -> Result<AgentRunPortSnapshot, AgentRunPortError> {
        let _command_guard = self.command_lock.lock().await;
        let snapshot = self.query(agent_run_id).await?;
        if snapshot.state.status.is_terminal() {
            return Ok(snapshot);
        }
        if snapshot.state.status != AgentRunStatus::Pending {
            return Err(AgentRunPortError::Rejected(format!(
                "cannot fail setup gate for active AgentRun {agent_run_id}"
            )));
        }

        let (request, attempt) = self.load_request(agent_run_id).await?;
        self.terminalize_failure(
            &request,
            &attempt,
            AgentRunStatus::Failed,
            AgentRuntimeError::new(AgentRuntimeErrorKind::StartupFailed, message),
        )
        .await;
        self.query(agent_run_id).await
    }

    /// Attach to one persisted process host and replay observations after the
    /// durable cursor. This is useful to startup orchestration and tests that
    /// need deterministic single-attempt reconstruction.
    pub async fn attach_process_host(&self, run_attempt_id: Uuid) -> Result<(), AgentRunPortError> {
        type HostAttachmentRow = (
            Json<AgentRunRequestEnvelope>,
            Json<RunAttemptRequest>,
            String,
            String,
            Option<String>,
            i64,
            Option<i64>,
        );
        let row: Option<HostAttachmentRow> = sqlx::query_as(
            r#"
            SELECT ar.request_envelope, ara.request_envelope,
                   apr.host_endpoint, apr.host_token, apr.host_instance_id,
                   apr.last_host_event_sequence, apr.host_protocol_version
            FROM agent_process_registry apr
            JOIN agent_run_attempts ara ON ara.id = apr.run_attempt_id
            JOIN agent_runs ar ON ar.id = ara.agent_run_id
            WHERE apr.run_attempt_id = ?
              AND apr.host_endpoint IS NOT NULL AND apr.host_token IS NOT NULL
            "#,
        )
        .bind(run_attempt_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        let Some((request, attempt, endpoint, token, host_instance_id, cursor, protocol)) = row
        else {
            return Err(AgentRunPortError::NotFound(run_attempt_id));
        };
        if !matches!(protocol, None | Some(1) | Some(2)) {
            return Err(AgentRunPortError::Rejected(
                "unsupported process host protocol".into(),
            ));
        }
        let after_sequence = u64::try_from(cursor).unwrap_or_default();
        let response = match send_host_command(
            &endpoint,
            &token,
            HostCommand::Attach { after_sequence },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                AgentRunRecord::mark_process_host_unreachable(&self.db.pool, run_attempt_id)
                    .await
                    .map_err(|mark_error| AgentRunPortError::Unavailable(mark_error.to_string()))?;
                return Err(AgentRunPortError::Unavailable(format!(
                    "process host unavailable for RunAttempt {run_attempt_id}: {error}"
                )));
            }
        };
        if let Some(error) = response.error {
            AgentRunRecord::mark_process_host_unreachable(&self.db.pool, run_attempt_id)
                .await
                .map_err(|mark_error| AgentRunPortError::Unavailable(mark_error.to_string()))?;
            return Err(AgentRunPortError::Unavailable(error));
        }
        if let Some(expected) = host_instance_id
            && response.host_instance_id.to_string() != expected
        {
            return Err(AgentRunPortError::Unavailable(
                "process host identity did not match persisted reservation".to_string(),
            ));
        }
        self.apply_host_events(&request.0, &attempt.0, response.events)
            .await?;
        let port = self.clone();
        tokio::spawn(async move {
            port.observe_process_host(request.0, attempt.0).await;
        });
        Ok(())
    }

    async fn cleanup_failed_process_host_launch(
        &self,
        run_attempt_id: Uuid,
        child: Option<&mut tokio::process::Child>,
    ) {
        if let Some(child) = child {
            let _ = child.kill().await;
            if let Err(error) = child.wait().await {
                tracing::warn!(run_attempt_id = %run_attempt_id, %error, "startup cleanup could not confirm host exit; retaining reservation");
                return;
            }
        }
        if let Err(error) =
            AgentRunRecord::clear_process_host_reservation(&self.db.pool, run_attempt_id).await
        {
            tracing::warn!(run_attempt_id = %run_attempt_id, %error, "failed to clear process-host reservation after launch failure");
        }
    }

    async fn append_lifecycle(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        status: AgentRunStatus,
    ) -> Result<(), AgentRunPortError> {
        self.append_event(
            request,
            attempt,
            AgentEventPayload::LifecycleChanged { status },
            Vec::new(),
            Utc::now(),
            None,
        )
        .await
    }

    async fn append_event(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        payload: AgentEventPayload,
        native_refs: Vec<NativeAuditReference>,
        timestamp: chrono::DateTime<Utc>,
        event_id: Option<Uuid>,
    ) -> Result<(), AgentRunPortError> {
        let _guard = self.event_write_lock.lock().await;
        let event_id = event_id.unwrap_or_else(Uuid::new_v4);
        // The orchestration identity is owned by the durable link table, not
        // by provider transports. Resolve it at the canonical write boundary
        // so every emitted AgentEvent can be ingested by orchestration after a
        // restart (direct runs simply have no link and remain unscoped).
        let orchestration_identity: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT orchestration_run_id, node_execution_id FROM orchestration_agent_run_links WHERE agent_run_id = ?",
        )
        .bind(request.agent_run_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        let (orchestration_run_id, orchestration_node_execution_id) = orchestration_identity
            .map(|(run_id, node_id)| (Some(run_id), Some(node_id)))
            .unwrap_or((None, None));
        let existing: Option<Json<AgentEventEnvelope>> =
            sqlx::query_scalar("SELECT event_envelope FROM agent_events WHERE event_id = ?")
                .bind(event_id)
                .fetch_optional(&self.db.pool)
                .await
                .map_err(port_database)?;
        if let Some(existing) = existing {
            let event = AgentEventEnvelope {
                schema_version: AGENT_EVENT_SCHEMA_VERSION,
                payload_version: AGENT_EVENT_PAYLOAD_VERSION,
                event_id,
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                turn_id: request.turn_id,
                run_attempt_id: attempt.run_attempt_id,
                run_attempt_number: attempt.attempt_number,
                sequence: existing.0.sequence,
                correlation_id: request.correlation_id,
                orchestration_run_id,
                orchestration_node_execution_id,
                timestamp,
                native_refs,
                payload,
            };
            return if event == existing.0 {
                Ok(())
            } else {
                Err(AgentRunPortError::Rejected(format!(
                    "canonical event id {event_id} was reused with different content"
                )))
            };
        }
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM agent_events WHERE run_attempt_id = ?",
        )
        .bind(attempt.run_attempt_id)
        .fetch_one(&self.db.pool)
        .await
        .map_err(port_database)?;
        let sequence = u64::try_from(sequence)
            .map_err(|_| AgentRunPortError::Rejected("invalid event sequence".to_string()))?;
        let event = AgentEventEnvelope {
            schema_version: AGENT_EVENT_SCHEMA_VERSION,
            payload_version: AGENT_EVENT_PAYLOAD_VERSION,
            event_id,
            session_id: request.session_id,
            agent_run_id: request.agent_run_id,
            turn_id: request.turn_id,
            run_attempt_id: attempt.run_attempt_id,
            run_attempt_number: attempt.attempt_number,
            sequence,
            correlation_id: request.correlation_id,
            orchestration_run_id,
            orchestration_node_execution_id,
            timestamp,
            native_refs,
            payload,
        };
        AgentEventRecord::append_and_project(&self.db.pool, &event)
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
        let terminal_event = match &event.payload {
            AgentEventPayload::LifecycleChanged { status } if status.is_terminal() => {
                Some(AgentRunTerminalEvent {
                    agent_run_id: request.agent_run_id,
                    session_id: request.session_id,
                    status: *status,
                })
            }
            _ => None,
        };
        let sender = self.sender(request.agent_run_id).await;
        let _ = sender.send(event);
        if let Some(terminal_event) = terminal_event {
            let _ = self.terminal_event_sender.send(terminal_event);
        }
        Ok(())
    }

    fn staged_event(
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        payload: AgentEventPayload,
        native_refs: Vec<NativeAuditReference>,
        timestamp: chrono::DateTime<Utc>,
        event_id: Option<Uuid>,
        orchestration_identity: (Option<Uuid>, Option<Uuid>),
    ) -> AgentEventEnvelope {
        AgentEventEnvelope {
            schema_version: AGENT_EVENT_SCHEMA_VERSION,
            payload_version: AGENT_EVENT_PAYLOAD_VERSION,
            event_id: event_id.unwrap_or_else(Uuid::new_v4),
            session_id: request.session_id,
            agent_run_id: request.agent_run_id,
            turn_id: request.turn_id,
            run_attempt_id: attempt.run_attempt_id,
            run_attempt_number: attempt.attempt_number,
            // Replaced atomically with a dense canonical sequence by the
            // host-batch persistence method.
            sequence: 0,
            correlation_id: request.correlation_id,
            orchestration_run_id: orchestration_identity.0,
            orchestration_node_execution_id: orchestration_identity.1,
            timestamp,
            native_refs,
            payload,
        }
    }

    async fn append_recoverable(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        payload: AgentEventPayload,
        native_refs: Vec<NativeAuditReference>,
        timestamp: chrono::DateTime<Utc>,
        event_id: Option<Uuid>,
    ) {
        if let Err(error) = self
            .append_event(request, attempt, payload, native_refs, timestamp, event_id)
            .await
        {
            tracing::error!(
                agent_run_id = %request.agent_run_id,
                run_attempt_id = %attempt.run_attempt_id,
                %error,
                "canonical AgentRun projection failed; preserving Native Audit for replay"
            );
            if let Err(mark_error) =
                AgentRunRecord::mark_projection_degraded(&self.db.pool, request.agent_run_id).await
            {
                tracing::error!(
                    agent_run_id = %request.agent_run_id,
                    error = %mark_error,
                    "failed to mark AgentRun projection degraded"
                );
            }
        }
    }

    async fn sender(&self, agent_run_id: Uuid) -> broadcast::Sender<AgentEventEnvelope> {
        let mut senders = self.event_senders.write().await;
        senders
            .entry(agent_run_id)
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }

    async fn live_sender(&self, agent_run_id: Uuid) -> broadcast::Sender<AgentLiveEvent> {
        let mut senders = self.live_event_senders.write().await;
        senders
            .entry(agent_run_id)
            .or_insert_with(|| broadcast::channel(512).0)
            .clone()
    }

    async fn load_request(
        &self,
        agent_run_id: Uuid,
    ) -> Result<(AgentRunRequestEnvelope, RunAttemptRequest), AgentRunPortError> {
        let row: Option<(Json<AgentRunRequestEnvelope>, Json<RunAttemptRequest>)> = sqlx::query_as(
            r#"
                SELECT ar.request_envelope, ara.request_envelope
                FROM agent_runs ar
                JOIN agent_run_attempts ara ON ara.agent_run_id = ar.id
                WHERE ar.id = ?
                ORDER BY ara.attempt_number DESC
                LIMIT 1
                "#,
        )
        .bind(agent_run_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        row.map(|(request, attempt)| (request.0, attempt.0))
            .ok_or(AgentRunPortError::NotFound(agent_run_id))
    }

    fn provider(&self, provider_id: &str) -> Result<DirectProvider, AgentRunPortError> {
        match provider_id.replace('-', "_").to_ascii_lowercase().as_str() {
            "gemini" => Ok(DirectProvider::Gemini),
            "codex" => Ok(DirectProvider::Codex),
            "claude_code" | "claude" => Ok(DirectProvider::ClaudeCode),
            "oh_my_pi" | "omp" => Ok(DirectProvider::OhMyPi),
            _ => Err(AgentRunPortError::Rejected(format!(
                "unsupported provider {provider_id}"
            ))),
        }
    }

    async fn validate_workspace(
        &self,
        request: &AgentRunRequestEnvelope,
    ) -> Result<Workspace, AgentRunPortError> {
        let session = Session::find_by_id(&self.db.pool, request.session_id)
            .await
            .map_err(port_database)?
            .ok_or(AgentRunPortError::NotFound(request.session_id))?;
        if session.workspace_id != request.workspace.workspace_id {
            return Err(AgentRunPortError::Rejected(
                "session and AgentRun workspace do not match".to_string(),
            ));
        }
        db::models::integration::guard_agent_dispatch(
            &self.db.pool,
            request.workspace.workspace_id,
            request.session_id,
            request.correlation_id,
        )
        .await
        .map_err(port_database)?;
        let workspace = Workspace::find_by_id(&self.db.pool, request.workspace.workspace_id)
            .await
            .map_err(port_database)?
            .ok_or(AgentRunPortError::NotFound(request.workspace.workspace_id))?;
        let current_dir = Path::new(&request.workspace.path);
        services::services::workspace_usage::validate_agent_owner(
            &self.db.pool,
            &workspace,
            request,
            false,
        )
        .await
        .map_err(|error| AgentRunPortError::Rejected(format!("{error:#}")))?;
        if !current_dir.is_dir() {
            return Err(AgentRunPortError::Rejected(format!(
                "AgentRun workspace does not exist: {}",
                current_dir.display()
            )));
        }
        let persisted_dir = workspace.container_ref.as_deref().ok_or_else(|| {
            AgentRunPortError::Rejected(
                "AgentRun workspace has no persisted local path".to_string(),
            )
        })?;
        let requested_path = tokio::fs::canonicalize(current_dir)
            .await
            .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
        let persisted_path = tokio::fs::canonicalize(persisted_dir)
            .await
            .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
        if requested_path != persisted_path {
            return Err(AgentRunPortError::Rejected(format!(
                "AgentRun workspace path {} does not match persisted workspace path {}",
                current_dir.display(),
                Path::new(persisted_dir).display()
            )));
        }
        Ok(workspace)
    }

    async fn execution_env(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        workspace: &Workspace,
        provider: DirectProvider,
    ) -> Result<ExecutionEnv, AgentRunPortError> {
        services::services::workspace_usage::validate_agent_owner(
            &self.db.pool,
            workspace,
            request,
            true,
        )
        .await
        .map_err(|error| AgentRunPortError::Rejected(format!("{error:#}")))?;
        let repos = WorkspaceRepo::find_repos_for_workspace(&self.db.pool, workspace.id)
            .await
            .map_err(port_database)?;
        let shared_roots: Vec<_> = repos
            .iter()
            .flat_map(|repo| {
                workspace_manager::shared_resources::writable_roots(&repo.name, repo.id)
            })
            .collect();
        let mut memory_instructions = Vec::new();
        let mut openwiki_maintenance = false;
        let mut openwiki_reviewer = false;
        let parallel_policy = services::services::parallel_context::saved_context(
            &self.db.pool,
            workspace.id,
            &request.input.content,
        )
        .await
        .map_err(|error| AgentRunPortError::Rejected(format!("Card context: {error:#}")))?;
        let local_api_port = utils::port_file::read_port_file("vibe-kanban").await.ok();
        let integration_workspace =
            db::models::integration::is_integration_workspace(&self.db.pool, workspace.id)
                .await
                .map_err(port_database)?;
        let source_completion_allowed = !workspace.is_execution_only()
            && !integration_workspace
            && executors::executors::provider_adapter::memory_source_completion_allowed(
                provider,
                direct_intent(request.intent, attempt.mode),
                &request.input.content,
            );
        for repo in &repos {
            if let Some(store) =
                utils::repository_memory::RepositoryMemoryStore::existing_for_repository(
                    &repo.name, repo.id,
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?
            {
                let state = store
                    .state()
                    .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                if state.maintenance_workspace_id == Some(workspace.id) {
                    if let Some(owner) = &state.bootstrap {
                        let child = owner.child.as_ref().ok_or_else(|| {
                            AgentRunPortError::Rejected(
                                "Bootstrap has not delegated an active child".into(),
                            )
                        })?;
                        let bound: bool = sqlx::query_scalar(
                            "SELECT EXISTS(SELECT 1 FROM workflow_runs wr JOIN orchestration_node_executions ne ON ne.orchestration_run_id = wr.orchestration_run_id WHERE wr.id = ? AND wr.repository_id = ? AND wr.workspace_id = ? AND wr.issue_id IS NULL AND wr.status IN ('running','awaiting_human') AND ne.id = ? AND ne.node_key = ?)",
                        ).bind(owner.workflow_run_id).bind(repo.id).bind(workspace.id).bind(child.node_execution_id).bind(&child.node_id)
                            .fetch_one(&self.db.pool).await.map_err(port_database)?;
                        openwiki_reviewer =
                            bootstrap_child_is_reviewer(&state, request, attempt, provider, bound)?;
                        openwiki_maintenance = !openwiki_reviewer;
                    } else if state.maintenance_session_id != Some(request.session_id)
                        || state
                            .active_run_id
                            .is_some_and(|id| id != request.agent_run_id)
                        || !matches!(
                            state.status,
                            utils::repository_memory::RepositoryWikiStatus::Initializing
                                | utils::repository_memory::RepositoryWikiStatus::Reconciling
                        )
                    {
                        return Err(AgentRunPortError::Rejected("This workspace belongs to repository maintenance. Use Sync Wiki to start a fenced maintenance run.".into()));
                    } else {
                        services::services::openwiki::sync_input::validate(&store, repo.id, &state)
                            .map_err(|error| {
                                AgentRunPortError::Rejected(format!(
                                    "Invalid Sync input: {error:#}"
                                ))
                            })?;
                        openwiki_maintenance = true;
                    }
                } else if state.enabled
                    && source_completion_allowed
                    && !crate::container::should_disable_default_commit_for_workspace(
                        &self.db.pool,
                        workspace.id,
                    )
                    .await
                    .map_err(port_database)?
                {
                    services::services::repository_memory::complete_previous_coding_runs(
                        &self.db.pool,
                        &store,
                        workspace.id,
                        request.agent_run_id,
                    )
                    .await
                    .map_err(|error| {
                        AgentRunPortError::Rejected(format!("Repository memory: {error:#}"))
                    })?;
                }
            }
            let root = Path::new(&request.workspace.path).join(&repo.name);
            if !openwiki_maintenance
                && !openwiki_reviewer
                && let Some(policy) = &parallel_policy
            {
                memory_instructions.push(services::services::parallel_context::instructions(
                    repo,
                    workspace,
                    &root,
                    local_api_port,
                    policy,
                    attempt.provider_session.is_none() && !request.input.content.contains(policy),
                ));
            }
            let membership =
                WorkspaceRepo::find_by_workspace_and_repo_id(&self.db.pool, workspace.id, repo.id)
                    .await
                    .map_err(port_database)?;
            if let Some(membership) = membership {
                let context = services::services::repository_memory::begin_coding_run(
                    repo,
                    workspace,
                    request.agent_run_id,
                    &root,
                    &membership.target_branch,
                    source_completion_allowed,
                )
                .map_err(|error| {
                    AgentRunPortError::Rejected(format!("Repository memory: {error:#}"))
                })?;
                if let Some(context) = context {
                    memory_instructions.push(context.instructions());
                }
            }
        }
        let repo_names = repos.into_iter().map(|repo| repo.name).collect();
        let mut env = ExecutionEnv::new(
            RepoContext::new(PathBuf::from(&request.workspace.path), repo_names),
            false,
            String::new(),
        );
        env.insert("VK_WORKSPACE_ID", workspace.id.to_string());
        env.insert("VK_WORKSPACE_BRANCH", &workspace.branch);
        env.insert("VK_AGENT_RUN_ID", request.agent_run_id.to_string());
        env.insert("VK_RUN_ATTEMPT_ID", attempt.run_attempt_id.to_string());
        env.insert("OPENWIKI_TELEMETRY_DISABLED", "1");
        if openwiki_maintenance {
            env.insert("EVK_OPENWIKI_MAINTENANCE", "1");
        }
        if openwiki_reviewer {
            env.insert("EVK_OPENWIKI_REVIEWER", "1");
        }
        if !memory_instructions.is_empty() {
            env.insert(
                "EVK_REPOSITORY_MEMORY_INSTRUCTIONS",
                memory_instructions.join("\n\n"),
            );
        }
        env.insert(
            "EVK_SHARED_RESOURCE_ROOTS",
            serde_json::to_string(&shared_roots)
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?,
        );
        Ok(env)
    }

    async fn require_audit_writer(
        &self,
        attempt: &RunAttemptRequest,
    ) -> Result<(), AgentRunPortError> {
        if self
            .audit_writers
            .lock()
            .await
            .contains_key(&attempt.run_attempt_id)
        {
            Ok(())
        } else {
            Err(AgentRunPortError::Unavailable(
                "Native Audit writer is not attached".to_string(),
            ))
        }
    }

    async fn terminalize_failure(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        status: AgentRunStatus,
        error: AgentRuntimeError,
    ) {
        self.append_recoverable(
            request,
            attempt,
            AgentEventPayload::Error { error },
            Vec::new(),
            Utc::now(),
            None,
        )
        .await;
        self.append_recoverable(
            request,
            attempt,
            AgentEventPayload::LifecycleChanged { status },
            Vec::new(),
            Utc::now(),
            None,
        )
        .await;
    }

    async fn launch_process_host(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        provider: DirectProvider,
        launch: &FrozenDirectProviderLaunchSpec,
    ) -> Result<(), AgentRunPortError> {
        // A create/outbox acknowledgement can be lost after the durable host
        // reservation has been written.  Before spawning another host, always
        // try to reuse a still-reserved endpoint.  The reservation is the
        // idempotency fence: even when host_pid has not been persisted yet,
        // the existing host may already be alive and accepting its launch.
        let existing: Option<(String, String, Option<String>, String)> = sqlx::query_as(
            r#"
            SELECT host_endpoint, host_token, host_instance_id, registry_status
            FROM agent_process_registry
            WHERE run_attempt_id = ?
              AND host_endpoint IS NOT NULL AND host_token IS NOT NULL
            "#,
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        if let Some((endpoint, token, host_instance_id, registry_status)) = existing
            && registry_status == "reserved"
        {
            let versions = provider.versions();
            let audited_launch_payload = launch
                .audit_payload()
                .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
            let launch_request = HostLaunchRequest {
                run_attempt_id: attempt.run_attempt_id,
                provider,
                executor_config: launch.executor_config.clone(),
                intent: launch.intent,
                prompt: launch.prompt.clone(),
                provider_session: launch.provider_session.clone(),
                reset_to_message_id: launch.reset_to_message_id.clone(),
                selected_skills: launch.selected_skills.clone(),
                current_dir: launch.current_dir.clone(),
                env: HostExecutionEnv::from(&launch.env),
                audit_metadata: NativeAuditMetadata {
                    session_id: request.session_id,
                    agent_run_id: request.agent_run_id,
                    turn_id: request.turn_id,
                    run_attempt_id: attempt.run_attempt_id,
                    run_attempt_number: attempt.attempt_number,
                    provider_id: provider.id().to_string(),
                    runtime_profile_id: request.runtime_profile_id.clone(),
                    workspace_path: request.workspace.path.clone(),
                    runtime_version: versions.runtime.map(str::to_owned),
                    protocol_version: versions.protocol.map(str::to_owned),
                    adapter_version: versions.adapter.to_string(),
                    mapper_version: versions.mapper.to_string(),
                    created_at: Utc::now(),
                },
                canonical_input: request.input.clone(),
                correlation_id: request.correlation_id,
                audited_launch_payload,
            };
            match send_host_command(
                &endpoint,
                &token,
                HostCommand::Launch(Box::new(launch_request)),
            )
            .await
            {
                Ok(response) => {
                    if let Some(expected) = host_instance_id
                        && response.host_instance_id.to_string() != expected
                    {
                        return Err(AgentRunPortError::Unavailable(
                            "process host identity did not match persisted reservation".to_string(),
                        ));
                    }
                    if let Some(error) = response.error {
                        return Err(AgentRunPortError::Unavailable(error));
                    }
                    self.apply_host_events(request, attempt, response.events)
                        .await?;
                }
                Err(error) => {
                    // The launch may have reached the host even when its ACK
                    // was lost.  Preserve the reservation and let the durable
                    // observer/reconciliation loop attach later; never spawn a
                    // replacement process from an uncertain acknowledgement.
                    let port = self.clone();
                    let request = request.clone();
                    let run_attempt_id = attempt.run_attempt_id;
                    let attempt = attempt.clone();
                    tokio::spawn(async move {
                        port.observe_process_host(request, attempt).await;
                    });
                    tracing::debug!(
                        run_attempt_id = %run_attempt_id,
                        %error,
                        "existing process-host launch response unavailable; preserving reservation"
                    );
                    return Ok(());
                }
            }
            let port = self.clone();
            let request = request.clone();
            let attempt = attempt.clone();
            tokio::spawn(async move {
                port.observe_process_host(request, attempt).await;
            });
            return Ok(());
        }

        let host_instance_id = Uuid::new_v4();
        let mut token_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let auth_token = token_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let endpoint_reservation = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
        let requested_endpoint = endpoint_reservation
            .local_addr()
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?
            .to_string();
        drop(endpoint_reservation);
        AgentRunRecord::reserve_process_host(
            &self.db.pool,
            attempt.run_attempt_id,
            &requested_endpoint,
            &auth_token,
            host_instance_id,
        )
        .await
        .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
        let host_executable = match resolve_process_host_executable() {
            Ok(path) => path,
            Err(error) => {
                self.cleanup_failed_process_host_launch(attempt.run_attempt_id, None)
                    .await;
                return Err(error);
            }
        };
        let mut child = match Command::new(host_executable)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                self.cleanup_failed_process_host_launch(attempt.run_attempt_id, None)
                    .await;
                return Err(AgentRunPortError::Unavailable(error.to_string()));
            }
        };
        let host_pid = match child.id() {
            Some(pid) => pid,
            None => {
                self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                    .await;
                return Err(AgentRunPortError::Unavailable(
                    "process host has no OS pid".to_string(),
                ));
            }
        };
        let mut child_stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                    .await;
                return Err(AgentRunPortError::Unavailable(
                    "process host stdin is unavailable".to_string(),
                ));
            }
        };
        if let Err(error) = write_json_frame(
            &mut child_stdin,
            &HostBootstrap {
                protocol_version: HOST_PROTOCOL_VERSION,
                run_attempt_id: attempt.run_attempt_id,
                host_instance_id,
                auth_token: auth_token.clone(),
                requested_endpoint: requested_endpoint.clone(),
            },
        )
        .await
        {
            self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                .await;
            return Err(AgentRunPortError::Unavailable(error.to_string()));
        }
        drop(child_stdin);
        let mut child_stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                    .await;
                return Err(AgentRunPortError::Unavailable(
                    "process host stdout is unavailable".to_string(),
                ));
            }
        };
        let ready: HostReady = match read_json_frame(&mut child_stdout).await {
            Ok(ready) => ready,
            Err(error) => {
                self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                    .await;
                return Err(AgentRunPortError::Unavailable(error.to_string()));
            }
        };
        if ready.protocol_version != HOST_PROTOCOL_VERSION
            || ready.host_instance_id != host_instance_id
            || ready.host_pid != host_pid
        {
            self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                .await;
            return Err(AgentRunPortError::Unavailable(
                "process host handshake did not match the reservation".to_string(),
            ));
        }

        if ready.endpoint != requested_endpoint {
            self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                .await;
            return Err(AgentRunPortError::Unavailable(
                "process host bound a different endpoint than reserved".to_string(),
            ));
        }
        if let Err(error) = AgentRunRecord::mark_process_host_attached(
            &self.db.pool,
            attempt.run_attempt_id,
            host_pid,
            ready.protocol_version,
            ready.start_identity.as_deref(),
        )
        .await
        {
            self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                .await;
            return Err(AgentRunPortError::Unavailable(error.to_string()));
        }

        let versions = provider.versions();
        let launch_request = HostLaunchRequest {
            run_attempt_id: attempt.run_attempt_id,
            provider,
            executor_config: launch.executor_config.clone(),
            intent: launch.intent,
            prompt: launch.prompt.clone(),
            provider_session: launch.provider_session.clone(),
            reset_to_message_id: launch.reset_to_message_id.clone(),
            selected_skills: launch.selected_skills.clone(),
            current_dir: launch.current_dir.clone(),
            env: HostExecutionEnv::from(&launch.env),
            audit_metadata: NativeAuditMetadata {
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                turn_id: request.turn_id,
                run_attempt_id: attempt.run_attempt_id,
                run_attempt_number: attempt.attempt_number,
                provider_id: provider.id().to_string(),
                runtime_profile_id: request.runtime_profile_id.clone(),
                workspace_path: request.workspace.path.clone(),
                runtime_version: versions.runtime.map(str::to_owned),
                protocol_version: versions.protocol.map(str::to_owned),
                adapter_version: versions.adapter.to_string(),
                mapper_version: versions.mapper.to_string(),
                created_at: Utc::now(),
            },
            canonical_input: request.input.clone(),
            correlation_id: request.correlation_id,
            audited_launch_payload: match launch.audit_payload() {
                Ok(payload) => payload,
                Err(error) => {
                    self.cleanup_failed_process_host_launch(
                        attempt.run_attempt_id,
                        Some(&mut child),
                    )
                    .await;
                    return Err(AgentRunPortError::Unavailable(error.to_string()));
                }
            },
        };
        let response = send_host_command(
            &ready.endpoint,
            &auth_token,
            HostCommand::Launch(Box::new(launch_request)),
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                // The command may have reached the durable host even when its
                // response was lost. Preserve the reservation and reconnect
                // through the host event cursor instead of killing a provider
                // that may already be running.
                let run_attempt_id = attempt.run_attempt_id;
                let port = self.clone();
                let request = request.clone();
                let attempt = attempt.clone();
                tokio::spawn(async move {
                    port.observe_process_host(request, attempt).await;
                });
                tokio::spawn(async move {
                    if let Err(wait_error) = child.wait().await {
                        tracing::debug!(%wait_error, "process host reaper lost its child handle");
                    }
                });
                tracing::debug!(run_attempt_id = %run_attempt_id, %error, "process host launch response unavailable; preserving durable host attachment");
                return Ok(());
            }
        };
        if let Some(error) = response.error {
            self.cleanup_failed_process_host_launch(attempt.run_attempt_id, Some(&mut child))
                .await;
            return Err(AgentRunPortError::Unavailable(error));
        }
        self.apply_host_events(request, attempt, response.events)
            .await?;
        let port = self.clone();
        let request = request.clone();
        let attempt = attempt.clone();
        tokio::spawn(async move {
            port.observe_process_host(request, attempt).await;
        });
        tokio::spawn(async move {
            if let Err(error) = child.wait().await {
                tracing::debug!(%error, "process host reaper lost its child handle");
            }
        });
        Ok(())
    }

    async fn observe_process_host(
        &self,
        request: AgentRunRequestEnvelope,
        attempt: RunAttemptRequest,
    ) {
        if !self
            .observing_attempts
            .lock()
            .await
            .insert(attempt.run_attempt_id)
        {
            return;
        }
        self.observe_process_host_inner(&request, &attempt).await;
        self.observing_attempts
            .lock()
            .await
            .remove(&attempt.run_attempt_id);
    }

    async fn observe_process_host_inner(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
    ) {
        let mut consecutive_failures = 0usize;
        let mut subscription = None;
        loop {
            let attachment: HostObservation = match sqlx::query_as(
                "SELECT host_endpoint, host_token, host_instance_id, last_host_event_sequence, registry_status, host_pid, pid, process_group_id, host_protocol_version, host_start_identity FROM agent_process_registry WHERE run_attempt_id = ?"
            ).bind(attempt.run_attempt_id).fetch_optional(&self.db.pool).await {
                Ok(Some(row)) => row,
                Ok(None) => return,
                Err(error) => {
                    subscription = None;
                    tracing::warn!(%error, "cannot read process host cursor; retrying");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            if attachment.registry_status == "exited" {
                return;
            }
            let (Some(endpoint), Some(token), Some(instance)) = (
                &attachment.host_endpoint,
                &attachment.host_token,
                &attachment.host_instance_id,
            ) else {
                return;
            };
            let after_sequence = match u64::try_from(attachment.last_host_event_sequence) {
                Ok(cursor) => cursor,
                Err(_) => {
                    tracing::error!("invalid persisted host cursor");
                    return;
                }
            };
            let streaming =
                attachment.host_protocol_version == Some(i64::from(HOST_PROTOCOL_VERSION));
            let response = async {
                if !matches!(attachment.host_protocol_version, None | Some(1) | Some(2)) {
                    return Err(crate::transport::TransportError::Protocol(
                        "unsupported process host protocol".into(),
                    ));
                }
                let response = if streaming {
                    if subscription.is_none() {
                        subscription =
                            Some(subscribe_host_events(endpoint, token, after_sequence).await?);
                    }
                    read_host_subscription(subscription.as_mut().expect("subscription created"))
                        .await?
                } else {
                    // Existing hosts have no new protocol metadata. Keep their
                    // Attach path; never send an unsupported command optimistically.
                    send_host_command(endpoint, token, HostCommand::Attach { after_sequence })
                        .await?
                };
                if response.host_instance_id.to_string() != *instance {
                    return Err(crate::transport::TransportError::Protocol(
                        "host identity mismatch".into(),
                    ));
                }
                if let Some(error) = &response.error {
                    return Err(crate::transport::TransportError::Protocol(error.clone()));
                }
                Ok(response)
            }
            .await;
            match response {
                Ok(response) => {
                    consecutive_failures = 0;
                    let had_events = !response.events.is_empty();
                    let terminal = response.terminal;
                    if let Err(error) = self
                        .apply_host_events(request, attempt, response.events)
                        .await
                    {
                        // A failed DB transaction must replay from the durable
                        // cursor, never continue the stream past that batch.
                        subscription = None;
                        tracing::warn!(%error, "host batch projection failed; replaying");
                    } else if terminal {
                        return;
                    } else if streaming || had_events {
                        tokio::task::yield_now().await;
                        continue;
                    }
                }
                Err(error) => {
                    subscription = None;
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    let _ = AgentRunRecord::mark_process_host_unreachable(
                        &self.db.pool,
                        attempt.run_attempt_id,
                    )
                    .await;
                    let absent = self.host_is_confirmed_absent(&attachment).await;
                    if consecutive_failures >= 3 && absent {
                        match self
                            .recover_dead_process_host(request, attempt, &attachment)
                            .await
                        {
                            Ok(true) => return,
                            Ok(false) => {}
                            Err(recovery) => {
                                tracing::error!(run_attempt_id = %attempt.run_attempt_id, %recovery, "dead host recovery failed closed");
                                if matches!(recovery, AgentRunPortError::Rejected(_)) {
                                    let _ = self
                                        .record_host_recovery_failure(
                                            request,
                                            attempt,
                                            &recovery.to_string(),
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                    if consecutive_failures >= CANCELLING_OBSERVER_FAILURE_THRESHOLD
                        && self
                            .query(request.agent_run_id)
                            .await
                            .is_ok_and(|snapshot| {
                                snapshot.state.status == AgentRunStatus::Cancelling
                            })
                    {
                        let _ = self
                            .reconcile_cancel_after_host_failure(
                                request,
                                attempt,
                                &error.to_string(),
                                None,
                            )
                            .await;
                        return;
                    }
                }
            }
            let delay = 1u64 << consecutive_failures.min(3);
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    }

    async fn host_is_confirmed_absent(&self, host: &HostObservation) -> bool {
        let Some(pid) = host.host_pid.and_then(|value| u32::try_from(value).ok()) else {
            return false;
        };
        self.process_registry
            .observe_host(pid, host.host_start_identity.as_deref())
            .await
            .is_ok_and(|presence| presence == RegisteredProcessPresence::Exited)
    }

    async fn recover_dead_process_host(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        host: &HostObservation,
    ) -> Result<bool, AgentRunPortError> {
        self.recover_dead_process_host_in(request, attempt, host, &utils::assets::asset_dir())
            .await
    }

    async fn recover_dead_process_host_in(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        host: &HostObservation,
        asset_root: &Path,
    ) -> Result<bool, AgentRunPortError> {
        // Called from the observer only after repeated transport failures.
        // Recheck just before disk replay, and never replace-spawn the provider.
        if !self.host_is_confirmed_absent(host).await {
            return Ok(false);
        }
        let lock = self
            .cancellation_reconciliation_lock(attempt.run_attempt_id)
            .await;
        let _guard = lock.lock().await;
        let mut provider_pid = host.pid.and_then(|pid| u32::try_from(pid).ok());
        let mut provider_group = host
            .process_group_id
            .and_then(|pid| u32::try_from(pid).ok());
        let mut verified_terminal = false;
        let mut recovered_exit = (None, Utc::now());
        if !matches!(host.host_protocol_version, None | Some(1) | Some(2)) {
            return Err(AgentRunPortError::Rejected(
                "unsupported process host protocol; cannot infer replay format".into(),
            ));
        }
        if host.host_protocol_version == Some(i64::from(HOST_PROTOCOL_VERSION)) {
            let instance = host
                .host_instance_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(|| {
                    AgentRunPortError::Unavailable("missing host replay identity".into())
                })?;
            let mut replay = HostJournalReplay::open(
                &asset_root
                    .join("runtime/host-events")
                    .join(format!("{}-{instance}.v1.jsonl", attempt.run_attempt_id)),
                attempt.run_attempt_id,
                instance,
                self.load_host_cursor(attempt).await?,
            )
            .await
            .map_err(|error| {
                AgentRunPortError::Rejected(format!("host journal recovery: {error}"))
            })?;
            let directory = asset_root.join(utils::native_audit::attempt_relative_dir(
                request.session_id,
                request.agent_run_id,
                attempt.run_attempt_id,
            ));
            if replay.last_sequence > 0 {
                replay
                    .verify_audit(&directory, request.session_id, request.agent_run_id)
                    .await
                    .map_err(|error| {
                        AgentRunPortError::Rejected(format!("host journal audit proof: {error}"))
                    })?;
            }
            verified_terminal = replay.terminal.is_some();
            if let Some(event) = &replay.terminal
                && let HostEventPayload::Terminal { exit_code, .. } = event.payload
            {
                recovered_exit = (exit_code, event.timestamp);
            }
            if let Some(started) = &replay.started {
                let HostEventPayload::Started {
                    provider_pid: recorded,
                    process_group_id: recorded_group,
                    ..
                } = started.payload
                else {
                    unreachable!()
                };
                if provider_pid.is_some_and(|persisted| persisted != recorded)
                    || provider_group.is_some_and(|persisted| Some(persisted) != recorded_group)
                {
                    return Err(AgentRunPortError::Unavailable(
                        "journal provider identity differs from registry".into(),
                    ));
                }
                provider_pid = Some(recorded);
                provider_group = recorded_group;
                if !self
                    .provider_is_confirmed_absent(provider_pid, provider_group)
                    .await
                {
                    // Make a started child observable/cancellable if server died
                    // before receiving Started. Never publish a recovered terminal
                    // fact while that child may still be running.
                    if self.load_host_cursor(attempt).await? == 0 {
                        self.apply_host_events(request, attempt, vec![started.clone()])
                            .await?;
                    }
                    return Ok(false);
                }
            }
            if replay.incomplete_tail {
                tracing::warn!(run_attempt_id = %attempt.run_attempt_id, "discarding uncommitted final journal append; not claiming complete recovery");
            }
            loop {
                let page = replay
                    .next_page()
                    .await
                    .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
                if page.is_empty() {
                    break;
                }
                self.apply_host_events(request, attempt, page).await?;
            }
        }
        // A dead host alone says nothing about its child's lifetime. Observe,
        // do not kill. Normal audited Cancel remains the intervention path.
        let provider_absent = self
            .provider_is_confirmed_absent(provider_pid, provider_group)
            .await;
        if !provider_absent {
            return Ok(false);
        }
        if provider_pid.is_some() {
            AgentRunRecord::mark_process_exited(
                &self.db.pool,
                attempt.run_attempt_id,
                recovered_exit.0,
                recovered_exit.1,
            )
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
            self.process_registry
                .remove_runtime(attempt.run_attempt_id)
                .await
                .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
        }
        if !verified_terminal
            && self.current_terminal_status(request.agent_run_id).await
                == Some(AgentRunStatus::Succeeded)
        {
            // Legacy providers could project success before the Host had
            // closed Audit. Never reinterpret that display state as proof.
            self.record_host_recovery_failure(
                request,
                attempt,
                "Legacy terminal projection has no verified Host terminal/Audit completion proof",
            )
            .await?;
        } else if self
            .current_terminal_status(request.agent_run_id)
            .await
            .is_none()
        {
            self.terminalize_failure(request, attempt, AgentRunStatus::Crashed,
                AgentRuntimeError::new(AgentRuntimeErrorKind::ProcessCrashed,
                    "Process host and provider exited without a verified terminal observation; recovered committed evidence only")
                    .with_provider(Some(request.provider_id.as_str()))).await;
        }
        Ok(true)
    }

    async fn record_host_recovery_failure(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        reason: &str,
    ) -> Result<(), AgentRunPortError> {
        // No host cursor advancement. A single durable diagnostic remains
        // visible; transient DB contention is not a permanent replay failure.
        let snapshot = self.query(request.agent_run_id).await?;
        if snapshot.state.projection_status == ProjectionStatus::ProjectionDegraded {
            return Ok(());
        }
        self.append_event(
            request,
            attempt,
            AgentEventPayload::ProjectionDegraded {
                reason: reason.into(),
            },
            Vec::new(),
            Utc::now(),
            None,
        )
        .await?;
        Ok(())
    }

    async fn provider_is_confirmed_absent(
        &self,
        pid: Option<u32>,
        process_group_id: Option<u32>,
    ) -> bool {
        match pid {
            Some(pid) => self
                .process_registry
                .observe_provider(pid, process_group_id)
                .await
                .is_ok_and(|presence| presence == RegisteredProcessPresence::Exited),
            // For v2 this is permitted only after validated journal shows no
            // Started. Legacy/no-journal recovery never claims launch success.
            None => true,
        }
    }

    async fn apply_host_events(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        events: Vec<crate::process_host::HostEvent>,
    ) -> Result<(), AgentRunPortError> {
        let _guard = self.event_write_lock.lock().await;
        let cursor = self.load_host_cursor(attempt).await?;
        if events
            .iter()
            .any(|event| !crate::process_host::journal::identity(event, attempt.run_attempt_id))
        {
            return Err(AgentRunPortError::Rejected(
                "foreign attempt in host observations".into(),
            ));
        }
        let replayed_terminal = events.iter().find_map(|event| match &event.payload {
            HostEventPayload::Terminal { exit_code, .. } if event.sequence <= cursor => {
                Some((event.sequence, *exit_code, event.timestamp))
            }
            _ => None,
        });
        let events: Vec<_> = events
            .into_iter()
            .filter(|event| event.sequence > cursor)
            .collect();
        let Some(through_host_sequence) = events.last().map(|event| event.sequence) else {
            // The canonical transaction may have committed before a process
            // registry write/ACK failed. Retrying must finish those idempotent
            // side effects without re-inserting events or re-registering Started.
            drop(_guard);
            if let Some((sequence, exit_code, exited_at)) = replayed_terminal {
                let pid: Option<i64> = sqlx::query_scalar(
                    "SELECT pid FROM agent_process_registry WHERE run_attempt_id = ?",
                )
                .bind(attempt.run_attempt_id)
                .fetch_one(&self.db.pool)
                .await
                .map_err(port_database)?;
                self.finish_host_projection(
                    attempt,
                    pid.map(|_| (exit_code, exited_at)),
                    Some(sequence),
                )
                .await?;
            }
            return Ok(());
        };
        if events
            .first()
            .is_some_and(|event| event.sequence != cursor + 1)
            || events
                .windows(2)
                .any(|pair| pair[0].sequence + 1 != pair[1].sequence)
        {
            return Err(AgentRunPortError::Rejected(
                "process host observations must be contiguous with the durable cursor".to_string(),
            ));
        }
        if events
            .iter()
            .any(|event| !crate::process_host::journal::identity(event, attempt.run_attempt_id))
        {
            return Err(AgentRunPortError::Rejected(
                "foreign attempt in host observations".into(),
            ));
        }
        let orchestration_identity: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "SELECT orchestration_run_id, node_execution_id FROM orchestration_agent_run_links WHERE agent_run_id = ?",
        )
        .bind(request.agent_run_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?
        .unwrap_or((None, None));
        let mut durable_events = Vec::new();
        let mut live_events = Vec::new();
        let mut host_groups = Vec::new();
        let mut observed_sessions = Vec::new();
        let mut terminal_ack = None;
        let mut exited_process = None;
        let mut projected_status = self
            .query(request.agent_run_id)
            .await
            .ok()
            .map(|snapshot| snapshot.state.status);

        for event in events {
            let host_sequence = event.sequence;
            let durable_start = durable_events.len();
            let live_start = live_events.len();
            match event.payload {
                HostEventPayload::Started {
                    provider_pid,
                    process_group_id,
                    executable,
                    canonical_input_ref,
                    audit_manifest,
                } => {
                    self.ensure_audit_stream(&audit_manifest).await?;
                    AgentRunRecord::mark_process_started(
                        &self.db.pool,
                        attempt.run_attempt_id,
                        provider_pid,
                        process_group_id,
                        Some(&executable),
                        event.timestamp,
                    )
                    .await
                    .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
                    let registered = RegisteredAgentProcess::new(
                        attempt.run_attempt_id,
                        Some(request.session_id),
                        Some(request.workspace.workspace_id),
                        Some(request.provider_id.clone()),
                        provider_pid,
                        process_group_id,
                        Some(executable),
                    );
                    self.process_registry
                        .register(registered)
                        .await
                        .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
                    durable_events.push(Self::staged_event(
                        request,
                        attempt,
                        AgentEventPayload::Message {
                            message: request.input.clone(),
                            final_output: false,
                        },
                        vec![canonical_input_ref],
                        request.created_at,
                        Some(request.input.message_id),
                        orchestration_identity,
                    ));
                    durable_events.push(Self::staged_event(
                        request,
                        attempt,
                        AgentEventPayload::LifecycleChanged {
                            status: AgentRunStatus::Running,
                        },
                        Vec::new(),
                        event.timestamp,
                        Some(event.event_id),
                        orchestration_identity,
                    ));
                    projected_status = Some(AgentRunStatus::Running);
                }
                HostEventPayload::Mapped {
                    mut event,
                    native_ref,
                } => {
                    event.native_refs = vec![native_ref];
                    if let AgentEventPayload::SessionObserved { provider_session } = &event.payload
                    {
                        observed_sessions.push(provider_session.clone());
                    }
                    if should_stage_mapped_lifecycle(projected_status, &event.payload) {
                        if let AgentEventPayload::LifecycleChanged { status } = event.payload {
                            projected_status = Some(status);
                            event.payload = AgentEventPayload::LifecycleChanged { status };
                        }
                        durable_events.push(event);
                    }
                }
                HostEventPayload::Projected {
                    durable_events: projected,
                    live_events: live,
                    native_ref,
                } => {
                    live_events.extend(live);
                    for mut mapped in projected {
                        mapped.native_refs = vec![native_ref.clone()];
                        if let AgentEventPayload::SessionObserved { provider_session } =
                            &mapped.payload
                        {
                            observed_sessions.push(provider_session.clone());
                        }
                        if should_stage_mapped_lifecycle(projected_status, &mapped.payload) {
                            if let AgentEventPayload::LifecycleChanged { status } = mapped.payload {
                                projected_status = Some(status);
                                mapped.payload = AgentEventPayload::LifecycleChanged { status };
                            }
                            durable_events.push(mapped);
                        }
                    }
                }
                HostEventPayload::Terminal {
                    mut status,
                    error,
                    error_event_id,
                    exit_code,
                    audit_manifest,
                } => {
                    self.ensure_audit_stream(&audit_manifest).await?;
                    NativeAuditStreamRecord::finalize(&self.db.pool, &audit_manifest)
                        .await
                        .map_err(port_database)?;
                    if projected_status == Some(AgentRunStatus::Cancelling) {
                        let process_registry_status: Option<String> = sqlx::query_scalar(
                            "SELECT registry_status FROM agent_process_registry WHERE run_attempt_id = ?",
                        )
                        .bind(attempt.run_attempt_id)
                        .fetch_optional(&self.db.pool)
                        .await
                        .map_err(port_database)?;
                        let process_was_confirmed_exited =
                            process_registry_status.as_deref() == Some("exited");
                        status = prefer_cancellation_terminal(status, process_was_confirmed_exited);
                    }
                    // A late provider failure can race with an accepted user
                    // cancellation. Preserve it in Native Audit, but do not
                    // project it as the cancelled run's canonical last_error.
                    if status != AgentRunStatus::Cancelled
                        && let Some(error) = error
                    {
                        durable_events.push(Self::staged_event(
                            request,
                            attempt,
                            AgentEventPayload::Error { error },
                            Vec::new(),
                            event.timestamp,
                            error_event_id,
                            orchestration_identity,
                        ));
                    }
                    // A transient snapshot/read failure must not discard the
                    // host's durable terminal fact.  Append it and let the
                    // canonical projection path report degradation; when a
                    // successful read proves the run is already terminal,
                    // the reducer's terminal guard makes a duplicate safe.
                    let should_append_terminal =
                        projected_status.is_none_or(|status| !status.is_terminal());
                    if should_append_terminal {
                        durable_events.push(Self::staged_event(
                            request,
                            attempt,
                            AgentEventPayload::LifecycleChanged { status },
                            Vec::new(),
                            event.timestamp,
                            Some(event.event_id),
                            orchestration_identity,
                        ));
                        projected_status = Some(status);
                    }
                    // A provider can fail before a child is spawned. In that
                    // case the host emits a terminal event without Started,
                    // and the registry must remain `reserved` to satisfy its
                    // CHECK constraints. Once a PID exists, transition it to
                    // exited exactly once and remove the local projection.
                    let pid: Option<i64> = sqlx::query_scalar(
                        "SELECT pid FROM agent_process_registry WHERE run_attempt_id = ?",
                    )
                    .bind(attempt.run_attempt_id)
                    .fetch_optional(&self.db.pool)
                    .await
                    .map_err(port_database)?;
                    if pid.is_some() {
                        exited_process = Some((exit_code, event.timestamp));
                    }
                    terminal_ack = Some(event.sequence);
                }
            }
            host_groups.push((
                host_sequence,
                durable_events[durable_start..].to_vec(),
                live_events[live_start..].to_vec(),
            ));
        }

        let mut retry = 0u8;
        let inserted = loop {
            match AgentEventRecord::append_and_project_host_batch(
                &self.db.pool,
                &durable_events,
                &live_events,
                &observed_sessions,
                attempt.run_attempt_id,
                through_host_sequence,
            )
            .await
            {
                Ok(inserted) => break inserted,
                Err(error) if is_transient_sqlite_contention(&error) && retry < 1 => {
                    retry += 1;
                    tokio::time::sleep(Duration::from_millis(50 * u64::from(retry))).await;
                }
                Err(error) if is_transient_sqlite_contention(&error) => {
                    return Err(AgentRunPortError::Unavailable(format!(
                        "temporary SQLite contention while projecting host batch: {error}"
                    )));
                }
                Err(error) if is_non_retryable_projection_error(&error) => {
                    tracing::warn!(agent_run_id = %request.agent_run_id, %error, "host batch failed permanently; isolating the offending host observation");
                    let mut recovered = Vec::new();
                    let mut recovered_live = Vec::new();
                    for (host_sequence, group_events, group_live) in &host_groups {
                        match AgentEventRecord::append_and_project_host_batch(
                            &self.db.pool,
                            group_events,
                            group_live,
                            &group_events
                                .iter()
                                .filter_map(|event| match &event.payload {
                                    AgentEventPayload::SessionObserved { provider_session } => {
                                        Some(provider_session.clone())
                                    }
                                    _ => None,
                                })
                                .collect::<Vec<_>>(),
                            attempt.run_attempt_id,
                            *host_sequence,
                        )
                        .await
                        {
                            Ok(inserted) => {
                                recovered.extend(inserted);
                                recovered_live.extend(group_live.clone());
                            }
                            Err(group_error) if is_transient_sqlite_contention(&group_error) => {
                                return Err(AgentRunPortError::Unavailable(format!(
                                    "temporary SQLite contention while isolating host batch: {group_error}"
                                )));
                            }
                            Err(group_error) if is_non_retryable_projection_error(&group_error) => {
                                AgentRunRecord::record_projection_failure_and_advance_cursor(
                                    &self.db.pool,
                                    request.agent_run_id,
                                    attempt.run_attempt_id,
                                    *host_sequence,
                                    "non_retryable_projection",
                                    &group_error.to_string(),
                                )
                                .await
                                .map_err(|record_error| {
                                    AgentRunPortError::Unavailable(format!(
                                        "failed to record isolated projection failure: {record_error}"
                                    ))
                                })?;
                                tracing::error!(
                                    agent_run_id = %request.agent_run_id,
                                    run_attempt_id = %attempt.run_attempt_id,
                                    host_event_sequence = *host_sequence,
                                    error = %group_error,
                                    "quarantined non-retryable host observation; Native Audit remains authoritative"
                                );
                            }
                            Err(group_error) => {
                                return Err(AgentRunPortError::Unavailable(format!(
                                    "database failure while isolating host batch; cursor was not advanced: {group_error}"
                                )));
                            }
                        }
                    }
                    live_events = recovered_live;
                    break recovered;
                }
                Err(error) => {
                    return Err(AgentRunPortError::Unavailable(format!(
                        "database failure while projecting host batch; cursor was not advanced: {error}"
                    )));
                }
            }
        };
        drop(_guard);

        let sender = self.sender(request.agent_run_id).await;
        let live_sender = self.live_sender(request.agent_run_id).await;
        let mut inserted_by_id: HashMap<Uuid, AgentEventEnvelope> = inserted
            .into_iter()
            .map(|event| (event.event_id, event))
            .collect();
        let mut live_by_id: HashMap<Uuid, AgentLiveEvent> = live_events
            .into_iter()
            .map(|event| (event.event_id, event))
            .collect();
        // Deliver only after the batch commit, but retain host/native ordering
        // between transient progress and durable semantic events.
        for (_, group_events, group_live) in &host_groups {
            for staged in group_events {
                let Some(event) = inserted_by_id.remove(&staged.event_id) else {
                    continue;
                };
                let terminal_event = match &event.payload {
                    AgentEventPayload::LifecycleChanged { status } if status.is_terminal() => {
                        Some(AgentRunTerminalEvent {
                            agent_run_id: request.agent_run_id,
                            session_id: request.session_id,
                            status: *status,
                        })
                    }
                    _ => None,
                };
                let _ = sender.send(event);
                if let Some(terminal_event) = terminal_event {
                    let _ = self.terminal_event_sender.send(terminal_event);
                }
            }
            for staged in group_live {
                if let Some(event) = live_by_id.remove(&staged.event_id) {
                    let _ = live_sender.send(event);
                }
            }
        }
        debug_assert!(inserted_by_id.is_empty());
        debug_assert!(live_by_id.is_empty());

        self.finish_host_projection(attempt, exited_process, terminal_ack)
            .await
    }

    async fn finish_host_projection(
        &self,
        attempt: &RunAttemptRequest,
        exited_process: Option<(Option<i64>, chrono::DateTime<Utc>)>,
        terminal_ack: Option<u64>,
    ) -> Result<(), AgentRunPortError> {
        if let Some((exit_code, exited_at)) = exited_process {
            AgentRunRecord::mark_process_exited(
                &self.db.pool,
                attempt.run_attempt_id,
                exit_code,
                exited_at,
            )
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
            if let Err(error) = self
                .process_registry
                .remove_runtime(attempt.run_attempt_id)
                .await
            {
                tracing::warn!(run_attempt_id = %attempt.run_attempt_id, %error, "failed to remove exited process registry entry");
            }
        }
        if let Some(sequence) = terminal_ack
            && let Some((endpoint, token)) = self.load_host_attachment(attempt).await?
        {
            let _ = send_host_command(
                &endpoint,
                &token,
                HostCommand::AckTerminal {
                    through_sequence: sequence,
                },
            )
            .await;
        }
        Ok(())
    }

    async fn ensure_audit_stream(
        &self,
        manifest: &executors::runtime::NativeAuditManifest,
    ) -> Result<(), AgentRunPortError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM native_audit_streams WHERE run_attempt_id = ?)",
        )
        .bind(manifest.run_attempt_id)
        .fetch_one(&self.db.pool)
        .await
        .map_err(port_database)?;
        if !exists {
            NativeAuditStreamRecord::insert_open(&self.db.pool, manifest)
                .await
                .map_err(port_database)?;
        }
        Ok(())
    }

    async fn load_host_attachment(
        &self,
        attempt: &RunAttemptRequest,
    ) -> Result<Option<(String, String)>, AgentRunPortError> {
        sqlx::query_as(
            "SELECT host_endpoint, host_token FROM agent_process_registry WHERE run_attempt_id = ? AND host_endpoint IS NOT NULL AND host_token IS NOT NULL",
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&self.db.pool)
        .await
            .map_err(port_database)
    }

    async fn load_host_cursor(
        &self,
        attempt: &RunAttemptRequest,
    ) -> Result<u64, AgentRunPortError> {
        let cursor: Option<i64> = sqlx::query_scalar(
            "SELECT last_host_event_sequence FROM agent_process_registry WHERE run_attempt_id = ?",
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        Ok(cursor
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or_default())
    }

    async fn start_attempt(
        &self,
        request: AgentRunRequestEnvelope,
        attempt: RunAttemptRequest,
        workspace: Workspace,
    ) {
        {
            let mut launching = self.launching_attempts.lock().await;
            if !launching.insert(attempt.run_attempt_id) {
                return;
            }
        }
        let run_attempt_id = attempt.run_attempt_id;
        self.start_attempt_reserved(request, attempt, workspace)
            .await;
        self.launching_attempts.lock().await.remove(&run_attempt_id);
    }

    async fn start_attempt_reserved(
        &self,
        request: AgentRunRequestEnvelope,
        attempt: RunAttemptRequest,
        workspace: Workspace,
    ) {
        if self
            .children
            .read()
            .await
            .contains_key(&attempt.run_attempt_id)
        {
            return;
        }
        let registry: Option<(String, Option<i64>)> = sqlx::query_as(
            "SELECT registry_status, pid FROM agent_process_registry WHERE run_attempt_id = ?",
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&self.db.pool)
        .await
        .ok()
        .flatten();
        if let Some((status, pid)) = registry {
            if matches!(status.as_str(), "spawned" | "running" | "unreachable") && pid.is_some() {
                return;
            }
            if status == "exited" {
                return;
            }
        }

        let provider = match self.provider(&request.provider_id) {
            Ok(provider) => provider,
            Err(error) => {
                self.terminalize_failure(
                    &request,
                    &attempt,
                    AgentRunStatus::Failed,
                    AgentRuntimeError::new(AgentRuntimeErrorKind::StartupFailed, error.to_string()),
                )
                .await;
                return;
            }
        };
        let env = match self
            .execution_env(&request, &attempt, &workspace, provider)
            .await
        {
            Ok(env) => env,
            Err(error) => {
                self.terminalize_failure(
                    &request,
                    &attempt,
                    AgentRunStatus::Failed,
                    AgentRuntimeError::new(AgentRuntimeErrorKind::StartupFailed, error.to_string())
                        .with_provider(Some(provider.id())),
                )
                .await;
                return;
            }
        };
        let launch = match FrozenDirectProviderLaunchSpec::new(&request, &attempt, provider, env) {
            Ok(launch) => launch,
            Err(error) => {
                self.terminalize_failure(
                    &request,
                    &attempt,
                    AgentRunStatus::Failed,
                    AgentRuntimeError::new(AgentRuntimeErrorKind::StartupFailed, error.to_string())
                        .with_provider(Some(provider.id())),
                )
                .await;
                return;
            }
        };
        self.append_recoverable(
            &request,
            &attempt,
            AgentEventPayload::LifecycleChanged {
                status: AgentRunStatus::Starting,
            },
            Vec::new(),
            Utc::now(),
            None,
        )
        .await;

        if let Err(error) = self
            .launch_process_host(&request, &attempt, provider, &launch)
            .await
        {
            self.terminalize_failure(
                &request,
                &attempt,
                if matches!(error, AgentRunPortError::Unavailable(_)) {
                    AgentRunStatus::AuditFailed
                } else {
                    AgentRunStatus::Failed
                },
                AgentRuntimeError::new(AgentRuntimeErrorKind::StartupFailed, error.to_string())
                    .with_provider(Some(provider.id())),
            )
            .await;
        }
    }

    async fn validate_durable_command(
        &self,
        command: &AgentRunPortCommandEnvelope,
    ) -> Result<(), AgentRunPortError> {
        let stored: Option<Json<AgentRunPortCommandEnvelope>> = sqlx::query_scalar(
            r#"
            SELECT command_envelope FROM agent_run_commands
            WHERE command_id = ? AND idempotency_key = ?
            UNION ALL
            SELECT command_envelope FROM orchestration_outbox
            WHERE command_id = ? AND idempotency_key = ?
            LIMIT 1
            "#,
        )
        .bind(command.command_id)
        .bind(&command.idempotency_key)
        .bind(command.command_id)
        .bind(&command.idempotency_key)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        match stored {
            Some(stored) if stored.0 == *command => Ok(()),
            Some(_) => Err(AgentRunPortError::Rejected(
                "durable AgentRun command payload does not match delivery".to_string(),
            )),
            None => Err(AgentRunPortError::Rejected(
                "control command is not present in a durable command store".to_string(),
            )),
        }
    }

    async fn write_attached_control(
        &self,
        attempt: &RunAttemptRequest,
        bytes: &[u8],
        direct_control: DirectControl,
    ) -> Result<(), AgentRunPortError> {
        if let Some((endpoint, token)) = self.load_host_attachment(attempt).await? {
            let after_sequence = self.load_host_cursor(attempt).await?;
            let response = send_host_command(
                &endpoint,
                &token,
                HostCommand::Control {
                    bytes: bytes.to_vec(),
                    control: Some(direct_control),
                    cancel: false,
                    after_sequence,
                },
            )
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
            let (request, _) = self.load_request(attempt.agent_run_id).await?;
            self.apply_host_events(&request, attempt, response.events)
                .await?;
            return response
                .error
                .map_or(Ok(()), |error| Err(AgentRunPortError::Unavailable(error)));
        }
        Err(AgentRunPortError::Unavailable(
            "AgentRun process host attachment is not persisted".to_string(),
        ))
    }

    async fn cancel_attached_attempt(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
    ) -> Result<(), AgentRunPortError> {
        let provider = self.provider(&request.provider_id)?;
        let control = encode_control(
            provider,
            &attempt.capability_snapshot,
            DirectControl::Cancel,
        )
        .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
        if let Some((endpoint, token)) = self.load_host_attachment(attempt).await? {
            let after_sequence = self.load_host_cursor(attempt).await?;
            let response = match send_host_command(
                &endpoint,
                &token,
                HostCommand::Control {
                    bytes: control,
                    control: Some(DirectControl::Cancel),
                    cancel: true,
                    after_sequence,
                },
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    return self
                        .reconcile_cancel_after_host_failure(
                            request,
                            attempt,
                            &error.to_string(),
                            None,
                        )
                        .await;
                }
            };
            let response_error = response.error;
            if let Some(error) = response_error {
                return self
                    .reconcile_cancel_after_host_failure(
                        request,
                        attempt,
                        &error,
                        Some(response.events),
                    )
                    .await;
            }
            self.apply_host_events(request, attempt, response.events)
                .await?;
            if let Some(status) = self.current_terminal_status(request.agent_run_id).await {
                return cancellation_terminal_result(status);
            }
            return self
                .reconcile_cancel_after_host_failure(
                    request,
                    attempt,
                    "process host acknowledged cancellation without a terminal event",
                    None,
                )
                .await;
        }
        // No provider bytes were written without a durable host attachment.
        // Keep the fail-closed writer check, but do not record a synthetic
        // stdin frame that could be mistaken for an actual provider input.
        if let Err(error) = self.require_audit_writer(attempt).await {
            let message = error.to_string();
            self.terminalize_failure(
                request,
                attempt,
                AgentRunStatus::AuditFailed,
                AgentRuntimeError::new(AgentRuntimeErrorKind::Unknown, message.clone())
                    .with_provider(Some(provider.id())),
            )
            .await;
            return Err(AgentRunPortError::Unavailable(message));
        }
        Err(AgentRunPortError::Unavailable(
            "AgentRun process host attachment is not persisted".to_string(),
        ))
    }

    async fn current_terminal_status(&self, agent_run_id: Uuid) -> Option<AgentRunStatus> {
        self.query(agent_run_id)
            .await
            .ok()
            .map(|snapshot| snapshot.state.status)
            .filter(|status| status.is_terminal())
    }

    async fn replay_attached_host_once(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
    ) -> Result<Option<AgentRunStatus>, AgentRunPortError> {
        let Some((endpoint, token)) = self.load_host_attachment(attempt).await? else {
            return Err(AgentRunPortError::Unavailable(
                "AgentRun process host attachment is not persisted".to_string(),
            ));
        };
        let after_sequence = self.load_host_cursor(attempt).await?;
        let response = send_host_command(&endpoint, &token, HostCommand::Attach { after_sequence })
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
        let response_error = response.error;
        self.apply_host_events(request, attempt, response.events)
            .await?;
        let terminal = self.current_terminal_status(request.agent_run_id).await;
        if terminal.is_none()
            && let Some(error) = response_error
        {
            return Err(AgentRunPortError::Unavailable(error));
        }
        Ok(terminal)
    }

    async fn replay_cancel_terminal_until_deadline(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
    ) -> Option<AgentRunStatus> {
        let deadline = tokio::time::Instant::now() + CANCEL_HOST_REPLAY_DEADLINE;
        loop {
            if let Ok(Some(status)) = self.replay_attached_host_once(request, attempt).await {
                return Some(status);
            }
            if tokio::time::Instant::now() >= deadline {
                return self.current_terminal_status(request.agent_run_id).await;
            }
            tokio::time::sleep(CANCEL_HOST_REPLAY_INTERVAL).await;
        }
    }

    async fn prepare_cancellation_cleanup(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
    ) -> Result<CancellationCleanupPreparation, AgentRunPortError> {
        let persisted: Option<PersistedCancellationProcess> = sqlx::query_as(
            r#"
                SELECT registry_status, pid, process_group_id, executable
                FROM agent_process_registry
                WHERE run_attempt_id = ?
                "#,
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(port_database)?;
        let Some((status, pid, process_group_id, executable)) = persisted else {
            return Ok(CancellationCleanupPreparation::ProcessAbsenceUnconfirmed);
        };
        if status == "exited" {
            let _ = self
                .process_registry
                .remove_runtime(attempt.run_attempt_id)
                .await;
            return Ok(CancellationCleanupPreparation::ProcessAlreadyExited);
        }
        if self
            .process_registry
            .query_runtime(attempt.run_attempt_id)
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?
            .is_some()
        {
            return Ok(CancellationCleanupPreparation::OwnedProcessRegistered);
        }
        let Some(pid) = pid else {
            return Ok(CancellationCleanupPreparation::ProcessAbsenceUnconfirmed);
        };
        let pid = u32::try_from(pid).map_err(|error| {
            AgentRunPortError::Unavailable(format!("persisted provider pid is invalid: {error}"))
        })?;
        let process_group_id =
            process_group_id
                .map(u32::try_from)
                .transpose()
                .map_err(|error| {
                    AgentRunPortError::Unavailable(format!(
                        "persisted provider process group is invalid: {error}"
                    ))
                })?;
        self.process_registry
            .register(RegisteredAgentProcess::new(
                attempt.run_attempt_id,
                Some(request.session_id),
                Some(request.workspace.workspace_id),
                Some(request.provider_id.clone()),
                pid,
                process_group_id,
                executable,
            ))
            .await
            .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
        Ok(CancellationCleanupPreparation::OwnedProcessRegistered)
    }

    async fn reconcile_cancel_after_host_failure(
        &self,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        host_error: &str,
        initial_events: Option<Vec<crate::process_host::HostEvent>>,
    ) -> Result<(), AgentRunPortError> {
        // A user cancel and a background host observer can discover the same
        // transport failure concurrently. Serialize fallback per attempt so
        // the second caller reuses the first caller's terminal fact instead
        // of treating an already-cleaned registry as a failed cleanup.
        let reconciliation_lock = self
            .cancellation_reconciliation_lock(attempt.run_attempt_id)
            .await;
        let _reconciliation_guard = reconciliation_lock.lock().await;

        if let Some(status) = self.current_terminal_status(request.agent_run_id).await {
            return cancellation_terminal_result(status);
        }
        if let Err(error) =
            AgentRunRecord::mark_process_host_unreachable(&self.db.pool, attempt.run_attempt_id)
                .await
        {
            tracing::warn!(run_attempt_id = %attempt.run_attempt_id, %error, "failed to mark process host unreachable during cancellation fallback");
        }

        let preparation = match self.prepare_cancellation_cleanup(request, attempt).await {
            Ok(preparation) => preparation,
            Err(error) => {
                let message = format!(
                    "process host cancellation failed ({host_error}); cleanup preparation failed: {error}"
                );
                self.terminalize_failure(
                    request,
                    attempt,
                    AgentRunStatus::Crashed,
                    AgentRuntimeError::new(AgentRuntimeErrorKind::ProcessCrashed, message.clone())
                        .with_provider(Some(request.provider_id.clone())),
                )
                .await;
                return Err(AgentRunPortError::Unavailable(message));
            }
        };
        let cleanup = if preparation == CancellationCleanupPreparation::OwnedProcessRegistered {
            match self
                .process_registry
                .cleanup_runtime(attempt.run_attempt_id)
                .await
            {
                Ok(report) => Some(report),
                Err(error) => {
                    let message = format!(
                        "process host cancellation failed ({host_error}); registry cleanup failed: {error}"
                    );
                    self.terminalize_failure(
                        request,
                        attempt,
                        AgentRunStatus::Crashed,
                        AgentRuntimeError::new(
                            AgentRuntimeErrorKind::ProcessCrashed,
                            message.clone(),
                        )
                        .with_provider(Some(request.provider_id.clone())),
                    )
                    .await;
                    return Err(AgentRunPortError::Unavailable(message));
                }
            }
        } else {
            None
        };
        let runtime_absent = preparation == CancellationCleanupPreparation::ProcessAlreadyExited
            || cleanup.is_some_and(|report| report.confirms_runtime_absent());
        if !runtime_absent {
            if let Some(events) = initial_events {
                self.apply_host_events(request, attempt, events).await?;
            }
            if let Some(status) = self.current_terminal_status(request.agent_run_id).await {
                return cancellation_terminal_result(status);
            }
            let message = format!(
                "process host cancellation failed ({host_error}); owned process cleanup did not confirm exit ({preparation:?}, report={cleanup:?})"
            );
            self.terminalize_failure(
                request,
                attempt,
                AgentRunStatus::Crashed,
                AgentRuntimeError::new(AgentRuntimeErrorKind::ProcessCrashed, message.clone())
                    .with_provider(Some(request.provider_id.clone())),
            )
            .await;
            return Err(AgentRunPortError::Unavailable(message));
        }

        if let Err(error) = AgentRunRecord::mark_process_exited(
            &self.db.pool,
            attempt.run_attempt_id,
            None,
            Utc::now(),
        )
        .await
        {
            tracing::warn!(run_attempt_id = %attempt.run_attempt_id, %error, "failed to persist cancellation fallback process exit");
        }
        if let Some(events) = initial_events {
            self.apply_host_events(request, attempt, events).await?;
            if let Some(status) = self.current_terminal_status(request.agent_run_id).await {
                return cancellation_terminal_result(status);
            }
        }
        if let Some(status) = self
            .replay_cancel_terminal_until_deadline(request, attempt)
            .await
        {
            return cancellation_terminal_result(status);
        }

        let message = format!(
            "provider process was terminated after process host failure ({host_error}), but Native Audit did not produce a terminal manifest"
        );
        self.terminalize_failure(
            request,
            attempt,
            AgentRunStatus::AuditFailed,
            AgentRuntimeError::new(AgentRuntimeErrorKind::Unknown, message.clone())
                .with_provider(Some(request.provider_id.clone())),
        )
        .await;
        Err(AgentRunPortError::Unavailable(message))
    }

    async fn cancellation_reconciliation_lock(&self, run_attempt_id: Uuid) -> Arc<Mutex<()>> {
        let mut locks = self.cancellation_reconciliation_locks.lock().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&run_attempt_id).and_then(Weak::upgrade) {
            return lock;
        }

        let lock = Arc::new(Mutex::new(()));
        locks.insert(run_attempt_id, Arc::downgrade(&lock));
        lock
    }
}

fn bootstrap_child_is_reviewer(
    state: &utils::repository_memory::RepositoryMemoryState,
    request: &AgentRunRequestEnvelope,
    attempt: &RunAttemptRequest,
    provider: DirectProvider,
    bound: bool,
) -> Result<bool, AgentRunPortError> {
    use utils::repository_memory::OpenWikiBootstrapPhase;
    let rejected = || {
        AgentRunPortError::Rejected(
            "AgentRun is not the delegated fresh Bootstrap child with the required role".into(),
        )
    };
    let owner = state.bootstrap.as_ref().ok_or_else(rejected)?;
    let child = owner.child.as_ref().ok_or_else(rejected)?;
    if !bound
        || child.session_id != request.session_id
        || child.agent_run_id != request.agent_run_id
        || state.active_run_id != Some(request.agent_run_id)
        || state.maintenance_session_id != Some(request.session_id)
        || request.intent != AgentRunIntent::Initial
        || attempt.mode != RunAttemptMode::Launch
        || provider != DirectProvider::Codex
    {
        return Err(rejected());
    }
    match (&owner.phase, child.node_id.as_str()) {
        (OpenWikiBootstrapPhase::Reviewing, "review")
            if attempt.selected_skills.as_ref().is_none_or(Vec::is_empty) =>
        {
            Ok(true)
        }
        (OpenWikiBootstrapPhase::Generating, "generate")
        | (OpenWikiBootstrapPhase::Refining, "refine") => Ok(false),
        _ => Err(rejected()),
    }
}

fn cancellation_terminal_result(status: AgentRunStatus) -> Result<(), AgentRunPortError> {
    if status == AgentRunStatus::Cancelled {
        Ok(())
    } else {
        Err(AgentRunPortError::Unavailable(format!(
            "cancellation converged to terminal status {status:?}"
        )))
    }
}

fn prefer_cancellation_terminal(
    provider_status: AgentRunStatus,
    process_was_confirmed_exited: bool,
) -> AgentRunStatus {
    match provider_status {
        AgentRunStatus::AuditFailed => AgentRunStatus::AuditFailed,
        AgentRunStatus::Cancelled | AgentRunStatus::Succeeded | AgentRunStatus::Failed => {
            AgentRunStatus::Cancelled
        }
        AgentRunStatus::Crashed if process_was_confirmed_exited => AgentRunStatus::Cancelled,
        status => status,
    }
}

#[async_trait]
impl AgentRunPort for LocalAgentRunPort {
    async fn create(
        &self,
        request: AgentRunRequestEnvelope,
        attempt: RunAttemptRequest,
    ) -> Result<Uuid, AgentRunPortError> {
        let agent_run_id = self.reserve(request, attempt).await?;
        self.launch_reserved(agent_run_id).await?;
        // Identity acceptance is the create acknowledgement. Launch/audit
        // failures are canonical terminal facts observed through query/subscribe.
        Ok(agent_run_id)
    }

    async fn query(&self, agent_run_id: Uuid) -> Result<AgentRunPortSnapshot, AgentRunPortError> {
        let state: Option<Json<executors::runtime::RunState>> =
            sqlx::query_scalar("SELECT state_json FROM agent_run_state WHERE agent_run_id = ?")
                .bind(agent_run_id)
                .fetch_optional(&self.db.pool)
                .await
                .map_err(port_database)?;
        state
            .map(|state| AgentRunPortSnapshot {
                agent_run_id,
                state: state.0,
            })
            .ok_or(AgentRunPortError::NotFound(agent_run_id))
    }

    async fn control(&self, command: AgentRunPortCommandEnvelope) -> Result<(), AgentRunPortError> {
        command
            .validate_current()
            .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
        self.validate_durable_command(&command).await?;
        if !matches!(&command.command, AgentRunPortCommand::Cancel { .. }) {
            let (request, _) = self.load_request(command.agent_run_id).await?;
            let workspace = Workspace::find_by_id(&self.db.pool, request.workspace.workspace_id)
                .await
                .map_err(port_database)?
                .ok_or(AgentRunPortError::NotFound(request.workspace.workspace_id))?;
            if workspace.is_execution_only() {
                if !matches!(
                    &command.command,
                    AgentRunPortCommand::Create { .. }
                        | AgentRunPortCommand::SubmitInput { .. }
                        | AgentRunPortCommand::ResolveApproval { .. }
                ) {
                    return Err(AgentRunPortError::Rejected("Execution-only workspace does not accept free-form controls or retries; use its owner".into()));
                }
                services::services::workspace_usage::validate_agent_owner(
                    &self.db.pool,
                    &workspace,
                    &request,
                    true,
                )
                .await
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
            }
            if let Some(owner) = db::models::integration::workspace_owner(
                &self.db.pool,
                request.workspace.workspace_id,
            )
            .await
            .map_err(port_database)?
                && !(command.orchestration_run_id == Some(owner)
                    || (request.correlation_id == owner
                        && matches!(
                            command.command,
                            AgentRunPortCommand::SubmitInput { .. }
                                | AgentRunPortCommand::ResolveApproval { .. }
                        )))
            {
                return Err(AgentRunPortError::Rejected(format!(
                    "Workspace reserved by Integration {owner}"
                )));
            }
        }
        // Serialize controls within this supervisor. Durable command identity
        // still protects retries across supervisor instances, while this lock
        // prevents cancel/retry/input races between local tasks.
        let _command_guard = self.command_lock.lock().await;
        let agent_run_id = command.agent_run_id;
        if !matches!(&command.command, AgentRunPortCommand::Cancel { .. })
            && self.query(agent_run_id).await?.state.projection_status != ProjectionStatus::Current
        {
            return Err(AgentRunPortError::Rejected(
                "canonical projection is degraded; only cancellation is allowed".to_string(),
            ));
        }
        match &command.command {
            AgentRunPortCommand::Cancel { .. } => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                let state = self.query(agent_run_id).await?;
                if state.state.status.is_terminal() {
                    return Ok(());
                }
                if state.state.status != AgentRunStatus::Cancelling {
                    self.append_lifecycle(&request, &attempt, AgentRunStatus::Cancelling)
                        .await?;
                }
                self.cancel_attached_attempt(&request, &attempt).await
            }
            AgentRunPortCommand::InterruptTurn => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot interrupt a terminal AgentRun".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let direct_control = DirectControl::Cancel;
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    direct_control.clone(),
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.write_attached_control(&attempt, &bytes, direct_control)
                    .await
            }
            AgentRunPortCommand::SubmitInput { input_id, content } => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot submit input to a terminal AgentRun".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    DirectControl::Input {
                        request_id: input_id.clone(),
                        text: content.clone(),
                    },
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.append_event(
                    &request,
                    &attempt,
                    AgentEventPayload::Message {
                        message: CanonicalMessage {
                            message_id: command.command_id,
                            role: AgentRuntimeMessageRole::User,
                            content: content.clone(),
                        },
                        final_output: false,
                    },
                    Vec::new(),
                    command.created_at,
                    Some(command.command_id),
                )
                .await?;
                self.write_attached_control(
                    &attempt,
                    &bytes,
                    DirectControl::Input {
                        request_id: input_id.clone(),
                        text: content.clone(),
                    },
                )
                .await?;
                self.append_event(
                    &request,
                    &attempt,
                    AgentEventPayload::InputResolved {
                        input_id: input_id.clone(),
                        answered: true,
                    },
                    Vec::new(),
                    Utc::now(),
                    None,
                )
                .await
            }
            AgentRunPortCommand::ResolveApproval {
                approval_id,
                approved,
                reason,
            } => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot resolve approval for a terminal AgentRun".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    DirectControl::Approve {
                        request_id: approval_id.clone(),
                        approved: *approved,
                        reason: reason.clone(),
                    },
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.write_attached_control(
                    &attempt,
                    &bytes,
                    DirectControl::Approve {
                        request_id: approval_id.clone(),
                        approved: *approved,
                        reason: reason.clone(),
                    },
                )
                .await?;
                self.append_event(
                    &request,
                    &attempt,
                    AgentEventPayload::ApprovalResolved {
                        approval_id: approval_id.clone(),
                        approved: *approved,
                        reason: reason.clone(),
                    },
                    Vec::new(),
                    Utc::now(),
                    None,
                )
                .await
            }
            AgentRunPortCommand::Steer { content } => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot steer a terminal AgentRun".to_string(),
                    ));
                }
                if content.trim().is_empty() {
                    return Err(AgentRunPortError::Rejected(
                        "steering content must not be empty".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let direct_control = DirectControl::Steer {
                    text: content.clone(),
                };
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    direct_control.clone(),
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.write_attached_control(&attempt, &bytes, direct_control)
                    .await?;
                self.append_event(
                    &request,
                    &attempt,
                    AgentEventPayload::Message {
                        message: CanonicalMessage {
                            message_id: command.command_id,
                            role: AgentRuntimeMessageRole::User,
                            content: content.clone(),
                        },
                        final_output: false,
                    },
                    Vec::new(),
                    command.created_at,
                    Some(command.command_id),
                )
                .await
            }
            AgentRunPortCommand::UpdatePlanGoalDraft { objective } => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot update a draft on a terminal AgentRun".to_string(),
                    ));
                }
                if objective.trim().is_empty() {
                    return Err(AgentRunPortError::Rejected(
                        "Plan with Goal objective must not be empty".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let direct_control = DirectControl::UpdatePlanGoalDraft {
                    objective: objective.clone(),
                };
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    direct_control.clone(),
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.write_attached_control(&attempt, &bytes, direct_control)
                    .await
            }
            AgentRunPortCommand::GoalUpdate {
                objective,
                status,
                token_budget,
            } => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot update the goal of a terminal AgentRun".to_string(),
                    ));
                }
                if objective
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(AgentRunPortError::Rejected(
                        "goal objective must not be empty".to_string(),
                    ));
                }
                if token_budget.flatten().is_some_and(|budget| budget <= 0) {
                    return Err(AgentRunPortError::Rejected(
                        "goal token budget must be positive".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let direct_control = DirectControl::GoalUpdate {
                    objective: objective.clone(),
                    status: *status,
                    token_budget: *token_budget,
                };
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    direct_control.clone(),
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.write_attached_control(&attempt, &bytes, direct_control)
                    .await
            }
            AgentRunPortCommand::GoalClear => {
                let (request, attempt) = self.load_request(agent_run_id).await?;
                if self.query(agent_run_id).await?.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot clear the goal of a terminal AgentRun".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let direct_control = DirectControl::GoalClear;
                let bytes = encode_control(
                    provider,
                    &attempt.capability_snapshot,
                    direct_control.clone(),
                )
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.write_attached_control(&attempt, &bytes, direct_control)
                    .await
            }
            AgentRunPortCommand::Retry {
                mode,
                run_attempt_id,
            } => {
                if *mode == RunAttemptMode::Launch {
                    return Err(AgentRunPortError::Rejected(
                        "retry mode must be resume or restart".to_string(),
                    ));
                }
                let (request, previous) = self.load_request(agent_run_id).await?;
                let state = self.query(agent_run_id).await?;
                if !state.state.status.is_terminal() {
                    return Err(AgentRunPortError::Rejected(
                        "cannot retry an active AgentRun".to_string(),
                    ));
                }
                let provider = self.provider(&request.provider_id)?;
                let provider_session = if *mode == RunAttemptMode::Resume {
                    require_capability(
                        provider,
                        &previous.capability_snapshot,
                        AgentCapability::SessionResume,
                        false,
                    )
                    .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                    Some(state.state.provider_session.clone().ok_or_else(|| {
                        AgentRunPortError::Rejected(
                            "resume retry requires an observed provider session".to_string(),
                        )
                    })?)
                } else {
                    None
                };
                let attempt = RunAttemptRequest {
                    schema_version: previous.schema_version,
                    payload_version: previous.payload_version,
                    request_id: command.command_id,
                    idempotency_key: command.idempotency_key.clone(),
                    session_id: previous.session_id,
                    agent_run_id: previous.agent_run_id,
                    turn_id: previous.turn_id,
                    run_attempt_id: *run_attempt_id,
                    attempt_number: previous.attempt_number + 1,
                    correlation_id: previous.correlation_id,
                    mode: *mode,
                    transport: previous.transport,
                    runtime_profile_id: previous.runtime_profile_id.clone(),
                    provider_id: previous.provider_id.clone(),
                    workspace: previous.workspace.clone(),
                    capability_snapshot: previous.capability_snapshot.clone(),
                    executor_config: previous.executor_config.clone(),
                    selected_skills: previous.selected_skills.clone(),
                    reset_to_message_id: previous.reset_to_message_id.clone(),
                    provider_session,
                    created_at: command.created_at,
                };
                let workspace = self.validate_workspace(&request).await?;
                AgentRunRecord::persist_retry_attempt_before_launch(
                    &self.db.pool,
                    &request,
                    &attempt,
                )
                .await
                .map_err(|error| AgentRunPortError::Rejected(error.to_string()))?;
                self.start_attempt(request, attempt, workspace).await;
                Ok(())
            }
            AgentRunPortCommand::Create { .. } => Err(AgentRunPortError::Rejected(
                "create commands must use AgentRunPort::create".to_string(),
            )),
        }
    }

    async fn subscribe(&self, agent_run_id: Uuid) -> Result<AgentEventStream, AgentRunPortError> {
        self.query(agent_run_id).await?;
        let receiver = self.sender(agent_run_id).await.subscribe();
        let history: Vec<Json<AgentEventEnvelope>> = sqlx::query_scalar(
            "SELECT event_envelope FROM agent_events WHERE agent_run_id = ? ORDER BY run_attempt_number, sequence",
        )
        .bind(agent_run_id)
        .fetch_all(&self.db.pool)
        .await
        .map_err(port_database)?;
        let history_event_ids: HashSet<Uuid> =
            history.iter().map(|event| event.0.event_id).collect();
        let live = stream::unfold(
            (receiver, history_event_ids),
            |(mut receiver, mut seen)| async move {
                loop {
                    match receiver.recv().await {
                        Ok(event) if seen.insert(event.event_id) => {
                            return Some((event, (receiver, seen)));
                        }
                        Ok(_) => continue,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
            },
        );
        Ok(stream::iter(history.into_iter().map(|event| event.0))
            .chain(live)
            .boxed())
    }
}

fn resolve_process_host_executable() -> Result<PathBuf, AgentRunPortError> {
    if let Some(path) = std::env::var_os("VIBE_KANBAN_AGENT_PROCESS_HOST") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(AgentRunPortError::Unavailable(format!(
            "configured process host does not exist: {}",
            path.display()
        )));
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_agent-process-host") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }
    let current_exe = std::env::current_exe()
        .map_err(|error| AgentRunPortError::Unavailable(error.to_string()))?;
    let file_name = if cfg!(windows) {
        "agent-process-host.exe"
    } else {
        "agent-process-host"
    };
    let sibling = current_exe.with_file_name(file_name);
    if sibling.is_file() {
        return Ok(sibling);
    }
    if current_exe
        .parent()
        .is_some_and(|directory| directory.ends_with("deps"))
        && let Some(target_directory) = current_exe.parent().and_then(Path::parent)
    {
        let target_sibling = target_directory.join(file_name);
        if target_sibling.is_file() {
            return Ok(target_sibling);
        }
    }
    Err(AgentRunPortError::Unavailable(format!(
        "agent process host executable was not found next to {}",
        current_exe.display()
    )))
}

fn port_database(error: sqlx::Error) -> AgentRunPortError {
    AgentRunPortError::Unavailable(error.to_string())
}

fn should_stage_mapped_lifecycle(
    current_status: Option<AgentRunStatus>,
    payload: &AgentEventPayload,
) -> bool {
    let AgentEventPayload::LifecycleChanged { status } = payload else {
        return true;
    };
    // A provider's turn-complete notification is not process/Audit completion.
    // Only HostEvent::Terminal may close the canonical run. In particular a
    // dead host with an unfinished audit must never recover as succeeded.
    if status.is_terminal() {
        return false;
    }
    current_status.is_none_or(|current| {
        !current.is_terminal()
            && (current != AgentRunStatus::Cancelling || *status == AgentRunStatus::Cancelled)
    })
}

fn is_transient_sqlite_contention(
    error: &db::models::agent_runtime::AgentRuntimePersistenceError,
) -> bool {
    let db::models::agent_runtime::AgentRuntimePersistenceError::Database(sqlx::Error::Database(
        database,
    )) = error
    else {
        return false;
    };
    database
        .code()
        .and_then(|code| code.parse::<i32>().ok())
        .is_some_and(|code| code & 0xff == 5 || code & 0xff == 6)
}

/// Only deterministic contract/reducer failures may be quarantined. Database
/// infrastructure failures are never converted into a skipped host cursor:
/// the next attach must replay them after the storage problem is repaired.
fn is_non_retryable_projection_error(
    error: &db::models::agent_runtime::AgentRuntimePersistenceError,
) -> bool {
    use db::models::agent_runtime::AgentRuntimePersistenceError;

    matches!(
        error,
        AgentRuntimePersistenceError::InvalidAttempt(_)
            | AgentRuntimePersistenceError::InvalidCommand(_)
            | AgentRuntimePersistenceError::InvalidVersion(_)
            | AgentRuntimePersistenceError::InvalidFirstAttempt
            | AgentRuntimePersistenceError::IdempotencyConflict { .. }
            | AgentRuntimePersistenceError::IdentityConflict { .. }
            | AgentRuntimePersistenceError::InvalidEnumEncoding(_)
            | AgentRuntimePersistenceError::Serialization(_)
            | AgentRuntimePersistenceError::Reducer(_)
            | AgentRuntimePersistenceError::MissingState(_)
            | AgentRuntimePersistenceError::MissingLaunchGate(_)
            | AgentRuntimePersistenceError::InvalidDirectCommand
    )
}

#[cfg(test)]
mod tests {
    use executors::runtime::{
        AGENT_REQUEST_PAYLOAD_VERSION, AGENT_REQUEST_SCHEMA_VERSION, AgentRuntimeMessageRole,
        AgentTransportKind, CanonicalMessage, WorkspaceMode, WorkspaceReference,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::TempDir;

    use super::*;

    async fn setup_runtime_db() -> DBService {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect sqlite");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("enable foreign keys");
        sqlx::raw_sql(
            r#"
            CREATE TABLE workspaces (id BLOB PRIMARY KEY);
            CREATE TABLE sessions (
                id BLOB PRIMARY KEY,
                workspace_id BLOB NOT NULL,
                FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("create anchor schema");
        sqlx::raw_sql(include_str!(
            "../../db/migrations/20260811000000_agent_runtime_v1.sql"
        ))
        .execute(&pool)
        .await
        .expect("create runtime schema");
        sqlx::raw_sql(include_str!(
            "../../db/migrations/20260909000000_agent_projection_failures.sql"
        ))
        .execute(&pool)
        .await
        .expect("create projection failure schema");
        sqlx::raw_sql(include_str!(
            "../../db/migrations/20260922010000_host_replay_identity.sql"
        ))
        .execute(&pool)
        .await
        .expect("host replay identity migration");
        DBService { pool }
    }

    async fn persisted_codex_run(db: &DBService) -> (AgentRunRequestEnvelope, RunAttemptRequest) {
        let workspace_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        sqlx::query("INSERT INTO workspaces (id) VALUES (?)")
            .bind(workspace_id)
            .execute(&db.pool)
            .await
            .expect("insert workspace");
        sqlx::query("INSERT INTO sessions (id, workspace_id) VALUES (?, ?)")
            .bind(session_id)
            .bind(workspace_id)
            .execute(&db.pool)
            .await
            .expect("insert session");

        let now = Utc::now();
        let workspace = WorkspaceReference {
            workspace_id,
            mode: WorkspaceMode::SharedWorkspace,
            path: "C:/workspace".to_string(),
        };
        let request = AgentRunRequestEnvelope {
            schema_version: AGENT_REQUEST_SCHEMA_VERSION,
            payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
            request_id: Uuid::new_v4(),
            idempotency_key: format!("fixture-run:{session_id}"),
            session_id,
            agent_run_id: Uuid::new_v4(),
            turn_id: Uuid::new_v4(),
            correlation_id: Uuid::new_v4(),
            intent: AgentRunIntent::Initial,
            runtime_profile_id: "CODEX:default".to_string(),
            provider_id: "codex".to_string(),
            workspace: workspace.clone(),
            input: CanonicalMessage {
                message_id: Uuid::new_v4(),
                role: AgentRuntimeMessageRole::User,
                content: "cancel me".to_string(),
            },
            created_at: now,
        };
        let attempt = RunAttemptRequest {
            schema_version: AGENT_REQUEST_SCHEMA_VERSION,
            payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
            request_id: Uuid::new_v4(),
            idempotency_key: format!("fixture-run:{session_id}:attempt:1"),
            session_id,
            agent_run_id: request.agent_run_id,
            turn_id: request.turn_id,
            run_attempt_id: Uuid::new_v4(),
            attempt_number: 1,
            correlation_id: request.correlation_id,
            mode: RunAttemptMode::Launch,
            transport: AgentTransportKind::AppServerJsonrpc,
            runtime_profile_id: request.runtime_profile_id.clone(),
            provider_id: request.provider_id.clone(),
            workspace,
            capability_snapshot: DirectProvider::Codex.capabilities("CODEX:default"),
            executor_config: executors::profile::ExecutorConfig {
                executor: executors::executors::BaseCodingAgent::Codex,
                variant: Some("default".to_string()),
                model_id: None,
                agent_id: None,
                reasoning_id: None,
                permission_policy: None,
                execution_mode: None,
                goal_token_budget: None,
                goal_max_concurrent_agents: None,
            },
            selected_skills: None,
            reset_to_message_id: None,
            provider_session: None,
            created_at: now,
        };
        AgentRunRecord::persist_identity_before_launch(&db.pool, &request, &attempt)
            .await
            .expect("persist AgentRun identity");
        (request, attempt)
    }

    fn test_execution_env(workspace_path: &str) -> ExecutionEnv {
        let mut env = ExecutionEnv::new(
            RepoContext::new(
                PathBuf::from(workspace_path),
                vec!["repository".to_string()],
            ),
            false,
            String::new(),
        );
        env.insert("VK_AGENT_RUN_ID", "frozen-run");
        env
    }

    async fn replay_fixture(
        root: &Path,
        request: &AgentRunRequestEnvelope,
        attempt: &RunAttemptRequest,
        provider_pid: u32,
        deltas: usize,
        terminal: bool,
    ) -> (
        HostObservation,
        PathBuf,
        Vec<crate::process_host::HostEvent>,
    ) {
        use executors::runtime::{NativeAuditChannel, NativeAuditDirection, NativeAuditFrame};

        use crate::process_host::{HostEvent, journal::HostJournal};
        let instance = Uuid::new_v4();
        let path = root
            .join("runtime/host-events")
            .join(format!("{}-{instance}.v1.jsonl", attempt.run_attempt_id));
        let mut journal = HostJournal::create(&path, attempt.run_attempt_id, instance)
            .await
            .unwrap();
        let mut writer = NativeAuditWriter::create_in(
            root,
            NativeAuditMetadata {
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                turn_id: request.turn_id,
                run_attempt_id: attempt.run_attempt_id,
                run_attempt_number: 1,
                provider_id: "codex".into(),
                runtime_profile_id: request.runtime_profile_id.clone(),
                workspace_path: attempt.workspace.path.clone(),
                runtime_version: None,
                protocol_version: None,
                adapter_version: "1".into(),
                mapper_version: "1".into(),
                created_at: Utc::now(),
            },
        )
        .unwrap();
        let reference = writer
            .append_canonical_input(&request.input, request.correlation_id)
            .unwrap();
        let mut events = vec![HostEvent {
            sequence: 1,
            event_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            payload: HostEventPayload::Started {
                provider_pid,
                process_group_id: None,
                executable: "fixture".into(),
                canonical_input_ref: reference,
                audit_manifest: writer.manifest().clone(),
            },
        }];
        journal.append(events[0].clone()).await.unwrap();
        for index in 0..=deltas {
            let value = if index == deltas {
                serde_json::json!({"method":"item/completed", "params":{"threadId":"fixture-root", "turnId":"fixture-turn", "item":{"id":"answer", "type":"agentMessage", "text":"completed semantic answer"}}})
            } else {
                serde_json::json!({"method":"item/agentMessage/delta", "params":{"threadId":"fixture-root", "turnId":"fixture-turn", "itemId":"answer", "delta":"streaming fragment"}})
            };
            let frame = NativeAuditFrame::from_bytes(
                index as u64 + 2,
                Utc::now(),
                NativeAuditDirection::Output,
                NativeAuditChannel::Stdout,
                "application/json",
                request.correlation_id,
                &serde_json::to_vec(&value).unwrap(),
            );
            let native_ref = writer.append(frame.clone()).unwrap();
            let executors::executors::provider_adapter::ProviderFrameClassification::Event {
                event,
                ..
            } = DirectProvider::Codex.classify_native_frame(&frame)
            else {
                panic!("fixture must classify")
            };
            let projection = DirectProvider::Codex
                .project_provider_event(&event, writer.manifest())
                .unwrap();
            let host_event = HostEvent {
                sequence: index as u64 + 2,
                event_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                payload: HostEventPayload::Projected {
                    durable_events: projection.durable_events,
                    live_events: projection.live_events,
                    native_ref,
                },
            };
            journal.append(host_event.clone()).await.unwrap();
            events.push(host_event);
        }
        if terminal {
            let event = HostEvent {
                sequence: deltas as u64 + 3,
                event_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                payload: HostEventPayload::Terminal {
                    status: AgentRunStatus::Succeeded,
                    error: None,
                    error_event_id: None,
                    exit_code: Some(0),
                    audit_manifest: writer.close().unwrap(),
                },
            };
            journal.append(event.clone()).await.unwrap();
            events.push(event);
        }
        journal.flush().await.unwrap();
        let host = HostObservation {
            host_endpoint: Some("127.0.0.1:1".into()),
            host_token: Some("fixture".into()),
            host_instance_id: Some(instance.to_string()),
            last_host_event_sequence: 0,
            registry_status: "reserved".into(),
            // A distinct start identity proves the original host exited without
            // touching the process which now occupies this PID.
            host_pid: Some(i64::from(std::process::id())),
            host_start_identity: Some("different-boot-and-start".into()),
            pid: None,
            process_group_id: None,
            host_protocol_version: Some(i64::from(HOST_PROTOCOL_VERSION)),
        };
        (host, path, events)
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn dead_host_replay_preserves_audit_compacts_deltas_and_is_idempotent() {
        use executors::runtime::AuditBundle;
        let root = TempDir::new().unwrap();
        let mut port = LocalAgentRunPort::new(setup_runtime_db().await);
        port.process_registry = AgentProcessRegistry::new(root.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (host, _, events) =
            replay_fixture(root.path(), &request, &attempt, i32::MAX as u32, 1000, true).await;
        let started = std::time::Instant::now();
        assert!(
            port.recover_dead_process_host_in(&request, &attempt, &host, root.path())
                .await
                .unwrap()
        );
        assert_eq!(
            port.query(request.agent_run_id).await.unwrap().state.status,
            AgentRunStatus::Succeeded
        );
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 1003);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE agent_run_id = ?")
                .bind(request.agent_run_id)
                .fetch_one(&port.db.pool)
                .await
                .unwrap();
        assert_eq!(
            count, 4,
            "initial user, running, completed assistant, terminal only"
        );
        let directory = root.path().join(utils::native_audit::attempt_relative_dir(
            request.session_id,
            request.agent_run_id,
            attempt.run_attempt_id,
        ));
        let audit = AuditBundle::read(&directory).unwrap();
        assert_eq!(audit.manifest().frame_count, 1002);
        // A server restart creates new in-memory observers but retains cursor.
        let mut restarted = LocalAgentRunPort::new(port.db.clone());
        restarted.process_registry = port.process_registry.clone();
        assert!(
            restarted
                .recover_dead_process_host_in(&request, &attempt, &host, root.path())
                .await
                .unwrap()
        );
        restarted
            .apply_host_events(&request, &attempt, events)
            .await
            .unwrap();
        let after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE agent_run_id = ?")
                .bind(request.agent_run_id)
                .fetch_one(&port.db.pool)
                .await
                .unwrap();
        assert_eq!(after, count);
        eprintln!(
            "host recovery fixture: raw=1002 canonical=4 replay_ms={}",
            started.elapsed().as_millis()
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn dead_host_does_not_complete_with_a_live_provider_and_recovers_partial_tail_as_crashed()
    {
        let root = TempDir::new().unwrap();
        let mut port = LocalAgentRunPort::new(setup_runtime_db().await);
        port.process_registry = AgentProcessRegistry::new(root.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (mut host, _, _) =
            replay_fixture(root.path(), &request, &attempt, std::process::id(), 0, true).await;
        assert!(
            !port
                .recover_dead_process_host_in(&request, &attempt, &host, root.path())
                .await
                .unwrap()
        );
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 1);
        assert_eq!(
            port.query(request.agent_run_id).await.unwrap().state.status,
            AgentRunStatus::Running
        );
        host.host_pid = None;
        assert!(!port.host_is_confirmed_absent(&host).await);
        host.host_pid = Some(i64::from(std::process::id()));
        host.host_start_identity =
            crate::agent_process_registry::process_start_identity(std::process::id()).unwrap();
        assert!(!port.host_is_confirmed_absent(&host).await);

        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (host, path, _) =
            replay_fixture(root.path(), &request, &attempt, i32::MAX as u32, 0, false).await;
        use tokio::io::AsyncWriteExt;
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .await
            .unwrap()
            .write_all(b"{unfinished")
            .await
            .unwrap();
        assert!(
            port.recover_dead_process_host_in(&request, &attempt, &host, root.path())
                .await
                .unwrap()
        );
        assert_eq!(
            port.query(request.agent_run_id).await.unwrap().state.status,
            AgentRunStatus::Crashed
        );
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn host_gaps_and_foreign_attempts_have_no_persistence_side_effects() {
        let root = TempDir::new().unwrap();
        let mut port = LocalAgentRunPort::new(setup_runtime_db().await);
        port.process_registry = AgentProcessRegistry::new(root.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (_, _, events) =
            replay_fixture(root.path(), &request, &attempt, i32::MAX as u32, 0, true).await;
        assert!(
            port.apply_host_events(&request, &attempt, events[1..].to_vec())
                .await
                .is_err()
        );
        let mut foreign = events.clone();
        if let HostEventPayload::Started { audit_manifest, .. } = &mut foreign[0].payload {
            audit_manifest.run_attempt_id = Uuid::new_v4();
        }
        assert!(
            port.apply_host_events(&request, &attempt, foreign)
                .await
                .is_err()
        );
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 0);
        assert!(port.process_registry.entries().await.unwrap().is_empty());
        assert!(!should_stage_mapped_lifecycle(
            Some(AgentRunStatus::Running),
            &AgentEventPayload::LifecycleChanged {
                status: AgentRunStatus::Succeeded
            }
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn replay_corruption_and_unknown_protocol_fail_closed_without_advancing_cursor() {
        let root = TempDir::new().unwrap();
        let mut port = LocalAgentRunPort::new(setup_runtime_db().await);
        port.process_registry = AgentProcessRegistry::new(root.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (mut host, path, _) =
            replay_fixture(root.path(), &request, &attempt, i32::MAX as u32, 0, true).await;
        host.host_protocol_version = Some(999);
        assert!(matches!(
            port.recover_dead_process_host_in(&request, &attempt, &host, root.path())
                .await,
            Err(AgentRunPortError::Rejected(_))
        ));
        host.host_protocol_version = Some(2);
        let mut bytes = tokio::fs::read(&path).await.unwrap();
        bytes[0] = b'!';
        tokio::fs::write(&path, bytes).await.unwrap();
        let error = port
            .recover_dead_process_host_in(&request, &attempt, &host, root.path())
            .await
            .unwrap_err();
        assert!(matches!(error, AgentRunPortError::Rejected(_)));
        port.record_host_recovery_failure(&request, &attempt, &error.to_string())
            .await
            .unwrap();
        port.record_host_recovery_failure(&request, &attempt, &error.to_string())
            .await
            .unwrap();
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 0);
        let state = port.query(request.agent_run_id).await.unwrap().state;
        assert_eq!(
            state.projection_status,
            ProjectionStatus::ProjectionDegraded
        );
        assert!(!state.status.is_terminal());
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE agent_run_id = ?")
                .bind(request.agent_run_id)
                .fetch_one(&port.db.pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "durable diagnostic is not spammed");
        assert!(port.process_registry.entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn host_projection_failure_retries_the_identical_page_without_cursor_loss() {
        let root = TempDir::new().unwrap();
        let mut port = LocalAgentRunPort::new(setup_runtime_db().await);
        port.process_registry = AgentProcessRegistry::new(root.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (_, _, events) =
            replay_fixture(root.path(), &request, &attempt, i32::MAX as u32, 0, true).await;
        sqlx::raw_sql("CREATE TRIGGER reject_event BEFORE INSERT ON agent_events BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;")
            .execute(&port.db.pool).await.unwrap();
        assert!(
            port.apply_host_events(&request, &attempt, events.clone())
                .await
                .is_err()
        );
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 0);
        assert_eq!(
            port.query(request.agent_run_id)
                .await
                .unwrap()
                .state
                .projection_status,
            ProjectionStatus::Current
        );
        sqlx::raw_sql("DROP TRIGGER reject_event")
            .execute(&port.db.pool)
            .await
            .unwrap();
        port.apply_host_events(&request, &attempt, events.clone())
            .await
            .unwrap();
        port.apply_host_events(&request, &attempt, events)
            .await
            .unwrap();
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 3);
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE agent_run_id = ?")
                .bind(request.agent_run_id)
                .fetch_one(&port.db.pool)
                .await
                .unwrap();
        assert_eq!(count, 4);
        assert!(port.process_registry.entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn replay_finishes_exit_side_effects_after_canonical_commit() {
        let root = TempDir::new().unwrap();
        let mut port = LocalAgentRunPort::new(setup_runtime_db().await);
        port.process_registry = AgentProcessRegistry::new(root.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let (_, _, events) =
            replay_fixture(root.path(), &request, &attempt, i32::MAX as u32, 0, true).await;
        sqlx::raw_sql("CREATE TRIGGER reject_exit BEFORE UPDATE ON agent_process_registry WHEN NEW.registry_status = 'exited' BEGIN SELECT RAISE(ABORT, 'fixture crash after canonical commit'); END;")
            .execute(&port.db.pool).await.unwrap();
        assert!(
            port.apply_host_events(&request, &attempt, events.clone())
                .await
                .is_err()
        );
        assert_eq!(port.load_host_cursor(&attempt).await.unwrap(), 3);
        sqlx::raw_sql("DROP TRIGGER reject_exit")
            .execute(&port.db.pool)
            .await
            .unwrap();
        port.apply_host_events(&request, &attempt, events)
            .await
            .unwrap();
        let (status, exit_code): (String, Option<i64>) = sqlx::query_as("SELECT registry_status, exit_code FROM agent_process_registry WHERE run_attempt_id = ?")
            .bind(attempt.run_attempt_id).fetch_one(&port.db.pool).await.unwrap();
        assert_eq!(status, "exited");
        assert_eq!(exit_code, Some(0));
        assert!(port.process_registry.entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bootstrap_delegation_requires_exact_fresh_child_and_phase() {
        use utils::repository_memory::{
            OpenWikiBootstrapChild, OpenWikiBootstrapOwner, OpenWikiBootstrapPhase,
            RepositoryMemoryState,
        };
        let db = setup_runtime_db().await;
        let (mut request, mut attempt) = persisted_codex_run(&db).await;
        let mut state = RepositoryMemoryState {
            active_run_id: Some(request.agent_run_id),
            maintenance_session_id: Some(request.session_id),
            bootstrap: Some(OpenWikiBootstrapOwner {
                workflow_run_id: Uuid::new_v4(),
                server_instance_id: Uuid::new_v4(),
                phase: OpenWikiBootstrapPhase::Reviewing,
                review_fingerprint: None,
                child: Some(OpenWikiBootstrapChild {
                    session_id: request.session_id,
                    agent_run_id: request.agent_run_id,
                    node_execution_id: Uuid::new_v4(),
                    node_id: "review".into(),
                }),
            }),
            ..Default::default()
        };
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .unwrap()
        );
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, false)
                .is_err()
        );
        attempt.mode = RunAttemptMode::Resume;
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .is_err()
        );
        attempt.mode = RunAttemptMode::Launch;
        request.intent = AgentRunIntent::FollowUp;
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .is_err()
        );
        request.intent = AgentRunIntent::Initial;
        state.bootstrap.as_mut().unwrap().phase = OpenWikiBootstrapPhase::CleaningUp;
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .is_err()
        );
        state.bootstrap.as_mut().unwrap().phase = OpenWikiBootstrapPhase::Generating;
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .is_err()
        );
        state
            .bootstrap
            .as_mut()
            .unwrap()
            .child
            .as_mut()
            .unwrap()
            .node_id = "generate".into();
        assert!(
            !bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .unwrap()
        );
        state.active_run_id = Some(Uuid::new_v4());
        assert!(
            bootstrap_child_is_reviewer(&state, &request, &attempt, DirectProvider::Codex, true)
                .is_err()
        );
    }

    #[tokio::test]
    async fn direct_launch_and_audit_use_frozen_attempt_inputs() {
        let db = setup_runtime_db().await;
        let port = LocalAgentRunPort::new(db);
        let (request, mut attempt) = persisted_codex_run(&port.db).await;
        let selected_skills = vec![api_types::SelectedSkill {
            name: "runtime-contracts".to_string(),
            path: PathBuf::from("C:/skills/runtime-contracts/SKILL.md"),
        }];
        attempt.executor_config.model_id = Some("openai/gpt-5.6-codex".to_string());
        attempt.executor_config.reasoning_id = Some("high".to_string());
        attempt.selected_skills = Some(selected_skills.clone());

        let launch = FrozenDirectProviderLaunchSpec::new(
            &request,
            &attempt,
            DirectProvider::Codex,
            test_execution_env(&attempt.workspace.path),
        )
        .unwrap();
        let direct = launch.launch_request();
        assert_eq!(direct.provider, DirectProvider::Codex);
        assert_eq!(direct.intent, DirectIntent::Initial);
        assert_eq!(direct.executor_config, &attempt.executor_config);
        assert_eq!(direct.prompt, request.input.content);
        assert_eq!(direct.selected_skills, selected_skills);
        assert_eq!(direct.current_dir, Path::new(&attempt.workspace.path));
        assert_eq!(
            direct.env.get("VK_AGENT_RUN_ID").map(String::as_str),
            Some("frozen-run")
        );

        let audited: serde_json::Value = serde_json::from_slice(
            &launch
                .audit_payload()
                .expect("frozen direct launch must serialize for audit"),
        )
        .expect("audit payload must be JSON");
        assert_eq!(audited["provider"], "codex");
        assert_eq!(audited["intent"], "initial");
        assert_eq!(audited["prompt"], request.input.content);
        assert_eq!(
            audited["executor_config"],
            serde_json::to_value(&attempt.executor_config).unwrap()
        );
        assert_eq!(
            audited["selected_skills"],
            serde_json::to_value(&selected_skills).unwrap()
        );
        assert_eq!(audited["approval_behavior"], "noop");
        assert_eq!(audited["current_dir"], attempt.workspace.path);
        assert_eq!(audited["env"]["vars"]["VK_AGENT_RUN_ID"], "frozen-run");
    }

    #[tokio::test]
    async fn direct_follow_up_uses_frozen_session_and_reset_target() {
        let db = setup_runtime_db().await;
        let port = LocalAgentRunPort::new(db);
        let (mut request, mut attempt) = persisted_codex_run(&port.db).await;
        request.intent = AgentRunIntent::FollowUp;
        request.runtime_profile_id = "CLAUDE_CODE:default".to_string();
        request.provider_id = "claude_code".to_string();
        attempt.runtime_profile_id = request.runtime_profile_id.clone();
        attempt.provider_id = request.provider_id.clone();
        attempt.executor_config = executors::profile::ExecutorConfig {
            executor: executors::executors::BaseCodingAgent::ClaudeCode,
            variant: Some("default".to_string()),
            model_id: Some("claude-sonnet-4".to_string()),
            agent_id: None,
            reasoning_id: None,
            permission_policy: None,
            execution_mode: None,
            goal_token_budget: None,
            goal_max_concurrent_agents: None,
        };
        attempt.reset_to_message_id = Some("message-17".to_string());
        attempt.provider_session = Some(executors::runtime::ProviderSessionReference {
            schema_version: executors::runtime::PROVIDER_SESSION_REFERENCE_SCHEMA_VERSION,
            provider_id: request.provider_id.clone(),
            runtime_profile_id: request.runtime_profile_id.clone(),
            provider_session_id: "claude-session".to_string(),
            observed_at: Utc::now(),
            metadata: None,
        });

        let launch = FrozenDirectProviderLaunchSpec::new(
            &request,
            &attempt,
            DirectProvider::ClaudeCode,
            test_execution_env(&attempt.workspace.path),
        )
        .unwrap();
        let direct = launch.launch_request();
        assert_eq!(direct.intent, DirectIntent::FollowUp);
        assert_eq!(direct.executor_config, &attempt.executor_config);
        assert_eq!(direct.reset_to_message_id, Some("message-17"));
        assert_eq!(
            direct
                .provider_session
                .map(|session| session.provider_session_id.as_str()),
            Some("claude-session")
        );
    }

    #[tokio::test]
    async fn launch_boundary_retires_old_cards_for_initial_follow_up_and_goal() {
        let db = setup_runtime_db().await;
        let (mut request, mut attempt) = persisted_codex_run(&db).await;
        let old_block = "<!-- vk:pipeline:start -->\n<!-- vk:pipeline:id=wikillm -->\n## Pipeline: LLM Wiki\nManual note: preserve API compatibility.\n<!-- vk:pipeline:end -->";
        for (intent, text) in [
            (AgentRunIntent::Initial, "Implement safely"),
            (AgentRunIntent::FollowUp, "Continue"),
            (AgentRunIntent::FollowUp, "/goal Implement safely"),
        ] {
            request.intent = intent;
            request.input.content = format!("{text}\n{old_block}");
            attempt.selected_skills = Some(vec![api_types::SelectedSkill {
                name: "knowledge-enrich".into(),
                path: utils::assets::asset_dir().join("skills/llm-wiki/knowledge-enrich/SKILL.md"),
            }]);
            let launch = FrozenDirectProviderLaunchSpec::new(
                &request,
                &attempt,
                DirectProvider::Codex,
                test_execution_env(&attempt.workspace.path),
            )
            .unwrap();
            let direct = launch.launch_request();
            assert!(direct.prompt.starts_with(text));
            assert!(
                direct
                    .prompt
                    .contains("Manual note: preserve API compatibility.")
            );
            assert!(!direct.prompt.contains("vk:pipeline"));
            assert!(direct.selected_skills.is_empty());
            let audited: serde_json::Value =
                serde_json::from_slice(&launch.audit_payload().unwrap()).unwrap();
            assert_eq!(audited["prompt"], direct.prompt);
            // Persisted caller input is not rewritten by compatibility filtering.
            assert!(request.input.content.contains(old_block));
        }
    }

    #[test]
    fn direct_intent_explicitly_combines_run_intent_and_attempt_mode() {
        let cases = [
            (
                AgentRunIntent::Initial,
                RunAttemptMode::Launch,
                DirectIntent::Initial,
            ),
            (
                AgentRunIntent::Initial,
                RunAttemptMode::Resume,
                DirectIntent::Resume,
            ),
            (
                AgentRunIntent::Initial,
                RunAttemptMode::Restart,
                DirectIntent::Initial,
            ),
            (
                AgentRunIntent::FollowUp,
                RunAttemptMode::Launch,
                DirectIntent::FollowUp,
            ),
            (
                AgentRunIntent::FollowUp,
                RunAttemptMode::Resume,
                DirectIntent::Resume,
            ),
            (
                AgentRunIntent::FollowUp,
                RunAttemptMode::Restart,
                DirectIntent::Initial,
            ),
            (
                AgentRunIntent::Review,
                RunAttemptMode::Launch,
                DirectIntent::Review,
            ),
            (
                AgentRunIntent::Review,
                RunAttemptMode::Resume,
                DirectIntent::Resume,
            ),
            (
                AgentRunIntent::Review,
                RunAttemptMode::Restart,
                DirectIntent::Review,
            ),
        ];

        for (intent, mode, expected) in cases {
            assert_eq!(direct_intent(intent, mode), expected);
        }
    }

    #[tokio::test]
    async fn cancel_audit_failure_has_no_provider_or_process_side_effects() {
        let db = setup_runtime_db().await;
        let port = LocalAgentRunPort::new(db);
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let process_before: Option<(Option<i64>, String)> = sqlx::query_as(
            "SELECT pid, registry_status FROM agent_process_registry WHERE run_attempt_id = ?",
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&port.db.pool)
        .await
        .unwrap();

        let error = port
            .cancel_attached_attempt(&request, &attempt)
            .await
            .expect_err("missing Native Audit writer must fail closed");

        assert!(matches!(error, AgentRunPortError::Unavailable(_)));
        assert_eq!(
            port.query(request.agent_run_id).await.unwrap().state.status,
            AgentRunStatus::AuditFailed
        );
        let process_after: Option<(Option<i64>, String)> = sqlx::query_as(
            "SELECT pid, registry_status FROM agent_process_registry WHERE run_attempt_id = ?",
        )
        .bind(attempt.run_attempt_id)
        .fetch_optional(&port.db.pool)
        .await
        .unwrap();
        assert_eq!(
            process_after, process_before,
            "audit failure must not mutate provider process ownership"
        );
        assert!(port.children.read().await.is_empty());
    }

    #[tokio::test]
    async fn cancellation_cleanup_restores_missing_file_registry_from_database() {
        let db = setup_runtime_db().await;
        let temp_dir = TempDir::new().expect("temp dir");
        let mut port = LocalAgentRunPort::new(db);
        port.process_registry = AgentProcessRegistry::new(temp_dir.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        AgentRunRecord::mark_process_started(
            &port.db.pool,
            attempt.run_attempt_id,
            4242,
            Some(4242),
            Some("codex"),
            Utc::now(),
        )
        .await
        .expect("mark provider running");

        let preparation = port
            .prepare_cancellation_cleanup(&request, &attempt)
            .await
            .expect("prepare cleanup");

        assert_eq!(
            preparation,
            CancellationCleanupPreparation::OwnedProcessRegistered
        );
        let restored = port
            .process_registry
            .query_runtime(attempt.run_attempt_id)
            .await
            .expect("query restored process")
            .expect("restored process");
        assert_eq!(restored.pid, 4242);
        assert_eq!(restored.process_group_id, Some(4242));
        assert_eq!(restored.provider.as_deref(), Some("codex"));
    }

    #[tokio::test]
    async fn cancellation_cleanup_trusts_prior_exit_fact_and_drops_stale_file_entry() {
        let db = setup_runtime_db().await;
        let temp_dir = TempDir::new().expect("temp dir");
        let mut port = LocalAgentRunPort::new(db);
        port.process_registry = AgentProcessRegistry::new(temp_dir.path().join("registry.json"));
        let (request, attempt) = persisted_codex_run(&port.db).await;
        AgentRunRecord::mark_process_started(
            &port.db.pool,
            attempt.run_attempt_id,
            4243,
            Some(4243),
            Some("codex"),
            Utc::now(),
        )
        .await
        .expect("mark provider running");
        AgentRunRecord::mark_process_exited(
            &port.db.pool,
            attempt.run_attempt_id,
            None,
            Utc::now(),
        )
        .await
        .expect("mark provider exited");
        port.process_registry
            .register(RegisteredAgentProcess::new(
                attempt.run_attempt_id,
                Some(request.session_id),
                Some(request.workspace.workspace_id),
                Some(request.provider_id.clone()),
                4243,
                Some(4243),
                Some("codex".to_string()),
            ))
            .await
            .expect("register stale projection");

        let preparation = port
            .prepare_cancellation_cleanup(&request, &attempt)
            .await
            .expect("prepare cleanup");

        assert_eq!(
            preparation,
            CancellationCleanupPreparation::ProcessAlreadyExited
        );
        assert!(
            port.process_registry
                .query_runtime(attempt.run_attempt_id)
                .await
                .expect("query registry")
                .is_none()
        );
    }

    #[test]
    fn accepted_cancellation_wins_over_late_provider_terminal_when_exit_is_proven() {
        assert_eq!(
            prefer_cancellation_terminal(AgentRunStatus::Succeeded, false),
            AgentRunStatus::Cancelled
        );
        assert_eq!(
            prefer_cancellation_terminal(AgentRunStatus::Failed, false),
            AgentRunStatus::Cancelled
        );
        assert_eq!(
            prefer_cancellation_terminal(AgentRunStatus::Crashed, true),
            AgentRunStatus::Cancelled
        );
        assert_eq!(
            prefer_cancellation_terminal(AgentRunStatus::Crashed, false),
            AgentRunStatus::Crashed
        );
        assert_eq!(
            prefer_cancellation_terminal(AgentRunStatus::AuditFailed, true),
            AgentRunStatus::AuditFailed
        );
    }

    #[tokio::test]
    async fn cancellation_reconciliation_is_serialized_per_attempt() {
        let db = setup_runtime_db().await;
        let port = LocalAgentRunPort::new(db);
        let run_attempt_id = Uuid::new_v4();

        let first = port.cancellation_reconciliation_lock(run_attempt_id).await;
        let same_attempt = port.cancellation_reconciliation_lock(run_attempt_id).await;
        let other_attempt = port.cancellation_reconciliation_lock(Uuid::new_v4()).await;

        assert!(Arc::ptr_eq(&first, &same_attempt));
        let first_guard = first.lock().await;
        assert!(same_attempt.try_lock().is_err());
        assert!(other_attempt.try_lock().is_ok());
        drop(first_guard);

        let old_lock = Arc::downgrade(&first);
        drop(first);
        drop(same_attempt);
        assert!(old_lock.upgrade().is_none());
        assert!(
            port.cancellation_reconciliation_lock(run_attempt_id)
                .await
                .try_lock()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn canonical_event_identity_is_idempotent_before_sequence_allocation() {
        let db = setup_runtime_db().await;
        let port = LocalAgentRunPort::new(db);
        let (request, attempt) = persisted_codex_run(&port.db).await;
        let event_id = Uuid::new_v4();
        let timestamp = Utc::now();
        let payload = AgentEventPayload::Message {
            message: CanonicalMessage {
                message_id: event_id,
                role: AgentRuntimeMessageRole::User,
                content: "answer the pending question".to_string(),
            },
            final_output: false,
        };

        port.append_event(
            &request,
            &attempt,
            payload.clone(),
            Vec::new(),
            timestamp,
            Some(event_id),
        )
        .await
        .expect("append canonical input message");
        port.append_event(
            &request,
            &attempt,
            payload,
            Vec::new(),
            timestamp,
            Some(event_id),
        )
        .await
        .expect("redelivered canonical input message is idempotent");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM agent_events WHERE agent_run_id = ? AND event_id = ?",
        )
        .bind(request.agent_run_id)
        .bind(event_id)
        .fetch_one(&port.db.pool)
        .await
        .expect("count canonical events");
        assert_eq!(count, 1);
        assert_eq!(
            port.query(request.agent_run_id)
                .await
                .expect("query AgentRun")
                .state
                .last_event_sequence,
            1
        );
    }
}
