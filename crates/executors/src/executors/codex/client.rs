use std::{
    collections::{HashMap, VecDeque},
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use codex_app_server_protocol::{
    ClientInfo, ClientNotification, ClientRequest, CommandExecutionApprovalDecision,
    CommandExecutionRequestApprovalResponse, ConfigBatchWriteParams, ConfigEdit, ConfigReadParams,
    ConfigReadResponse, ConfigWriteResponse, CurrentTimeReadResponse,
    DynamicToolCallOutputContentItem, DynamicToolCallResponse, FileChangeApprovalDecision,
    FileChangeRequestApprovalResponse, GetAccountParams, GetAccountRateLimitsResponse,
    GetAccountResponse, InitializeCapabilities, InitializeParams, InitializeResponse,
    ItemCompletedNotification, JSONRPCError, JSONRPCNotification, JSONRPCRequest, JSONRPCResponse,
    ListMcpServerStatusParams, ListMcpServerStatusResponse, McpServerElicitationAction,
    McpServerElicitationRequestResponse, McpServerStatusDetail, ModelListParams, ModelListResponse,
    RequestId, ReviewStartParams, ReviewStartResponse, ReviewTarget, ServerRequest,
    SkillsListParams, SkillsListResponse, ThreadCompactStartParams, ThreadCompactStartResponse,
    ThreadGoalClearParams, ThreadGoalClearResponse, ThreadGoalGetParams, ThreadGoalGetResponse,
    ThreadGoalSetParams, ThreadGoalSetResponse, ThreadGoalStatus, ThreadItem, ThreadReadParams,
    ThreadReadResponse, ThreadResumeParams, ThreadSettingsUpdateParams,
    ThreadSettingsUpdateResponse, ThreadStartParams, ThreadStartResponse,
    ToolRequestUserInputAnswer, ToolRequestUserInputQuestion, ToolRequestUserInputResponse,
    TurnInterruptParams, TurnStartParams, TurnStartResponse, TurnStatus, TurnSteerParams,
    TurnSteerResponse, UserInput,
};
use codex_protocol::{
    config_types::{CollaborationMode, ModeKind, Settings},
    openai_models::ReasoningEffort as ProtocolReasoningEffort,
};
use futures::TryFutureExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{self, Value};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt, BufWriter},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;
use workspace_utils::approvals::{ApprovalStatus, QuestionStatus};

use super::{
    goal_lifecycle::GoalLifecycle,
    jsonrpc::{JsonRpcCallbacks, JsonRpcControlFlow, JsonRpcPeer},
};
use crate::{
    approvals::{ExecutorApprovalError, ExecutorApprovalService},
    env::RepoContext,
    executors::{
        ExecutorControl, ExecutorError, ExecutorExitResult, codex::normalize_logs::Approval,
        provider_adapter::DirectControl,
    },
    profile::ExecutionMode,
};

struct PendingPlan {
    item_id: String,
    text: String,
}

// Resume is a control operation, not a history importer. Decode only the
// required control fields; new display-only history variants must not prevent
// resuming a valid provider thread. The complete response remains in Audit.
#[derive(Debug, Deserialize)]
pub struct ResumedThread {
    pub thread: ResumedThreadIdentity,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct ResumedThreadIdentity {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct CompletedTurnNotification {
    turn: CompletedTurn,
}

#[derive(Debug, Deserialize)]
struct CompletedTurn {
    id: String,
    status: TurnStatus,
    #[serde(default)]
    error: Option<Value>,
}

pub struct AppServerClient {
    rpc: OnceLock<JsonRpcPeer>,
    log_writer: LogWriter,
    approvals: Option<Arc<dyn ExecutorApprovalService>>,
    thread_id: Mutex<Option<String>>,
    turn_id: Arc<Mutex<Option<String>>>,
    pending_feedback: Mutex<VecDeque<String>>,
    auto_approve: bool,
    plan_mode: bool,
    execution_mode: ExecutionMode,
    goal_token_budget: Option<i64>,
    goal_lifecycle: StdMutex<GoalLifecycle>,
    self_ref: Weak<AppServerClient>,
    resolved_model: OnceLock<String>,
    reasoning_effort: StdMutex<Option<ProtocolReasoningEffort>>,
    pending_plan: Mutex<Option<PendingPlan>>,
    pending_plan_goal_draft: Mutex<Option<String>>,
    repo_context: RepoContext,
    commit_reminder: bool,
    commit_reminder_prompt: String,
    commit_reminder_sent: AtomicBool,
    cancel: CancellationToken,
}

impl std::fmt::Debug for AppServerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppServerClient")
            .finish_non_exhaustive()
    }
}

impl AppServerClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        log_writer: LogWriter,
        approvals: Option<Arc<dyn ExecutorApprovalService>>,
        auto_approve: bool,
        plan_mode: bool,
        execution_mode: ExecutionMode,
        goal_token_budget: Option<i64>,
        reasoning_effort: Option<ProtocolReasoningEffort>,
        repo_context: RepoContext,
        commit_reminder: bool,
        commit_reminder_prompt: String,
        cancel: CancellationToken,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            rpc: OnceLock::new(),
            log_writer,
            approvals,
            auto_approve,
            plan_mode,
            execution_mode,
            goal_token_budget,
            goal_lifecycle: StdMutex::new(GoalLifecycle::default()),
            self_ref: weak.clone(),
            resolved_model: OnceLock::new(),
            reasoning_effort: StdMutex::new(reasoning_effort),
            pending_plan: Mutex::new(None),
            pending_plan_goal_draft: Mutex::new(None),
            thread_id: Mutex::new(None),
            turn_id: Arc::new(Mutex::new(None)),
            pending_feedback: Mutex::new(VecDeque::new()),
            repo_context,
            commit_reminder,
            commit_reminder_prompt,
            commit_reminder_sent: AtomicBool::new(false),
            cancel,
        })
    }

    pub fn connect(&self, peer: JsonRpcPeer) {
        let _ = self.rpc.set(peer);
    }

    pub fn set_resolved_model(&self, model: String) {
        let _ = self.resolved_model.set(model);
    }

    fn rpc(&self) -> &JsonRpcPeer {
        self.rpc.get().expect("Codex RPC peer not attached")
    }

    pub fn log_writer(&self) -> &LogWriter {
        &self.log_writer
    }

    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    fn observes_goal_lifecycle(&self) -> bool {
        matches!(
            self.execution_mode,
            ExecutionMode::Goal | ExecutionMode::PlanWithGoal
        )
    }

    fn observe_goal_updated(&self, thread: &str, status: &str) -> Option<ExecutorExitResult> {
        if !self.observes_goal_lifecycle() {
            return None;
        }

        self.goal_lifecycle
            .lock()
            .unwrap()
            .observe(thread, status)
            .then_some(ExecutorExitResult::Success)
    }

    fn goal_keeps_run_alive(&self) -> bool {
        let state = self.goal_lifecycle.lock().unwrap();
        // A resumed thread may produce a turn before our Goal activation is
        // acknowledged. That turn cannot certify success of this launch.
        (self.execution_mode == ExecutionMode::Goal && state.is_observing()) || state.keep_alive()
    }

    pub async fn initialize(&self) -> Result<(), ExecutorError> {
        let request = ClientRequest::Initialize {
            request_id: self.next_request_id(),
            params: InitializeParams {
                client_info: ClientInfo {
                    name: "vibe-codex-executor".to_string(),
                    title: None,
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                capabilities: Some(InitializeCapabilities {
                    experimental_api: true,
                    ..Default::default()
                }),
            },
        };

        let response = self
            .send_request::<InitializeResponse>(request, "initialize")
            .await?;
        // `app-server` is a JSON-RPC integration boundary. The CLI version
        // reported in the initialize response is useful audit information,
        // but it is not a compatibility gate. Runtime compatibility is
        // established by the handshake and the methods/capabilities we use.
        observe_codex_version(&response.user_agent);
        self.send_message(&ClientNotification::Initialized).await
    }

    pub async fn thread_start(
        &self,
        params: ThreadStartParams,
    ) -> Result<ThreadStartResponse, ExecutorError> {
        let request = ClientRequest::ThreadStart {
            request_id: self.next_request_id(),
            params,
        };
        self.send_request(request, "thread/start").await
    }

    pub async fn thread_resume(
        &self,
        params: ThreadResumeParams,
    ) -> Result<ResumedThread, ExecutorError> {
        // A Vibe follow-up continues the same native Codex conversation.
        // `thread/fork` would create another CLI-visible thread on every turn.
        let requested_thread_id = params.thread_id.clone();
        *self.thread_id.lock().await = Some(requested_thread_id.clone());
        let request = ClientRequest::ThreadResume {
            request_id: self.next_request_id(),
            params,
        };
        let response: ResumedThread = self.send_request(request, "thread/resume").await?;
        ensure_resumed_thread_id(&requested_thread_id, &response.thread.id)?;
        Ok(response)
    }

    pub async fn turn_start_with_mode(
        &self,
        thread_id: String,
        input: Vec<UserInput>,
        collaboration_mode: Option<CollaborationMode>,
    ) -> Result<TurnStartResponse, ExecutorError> {
        let effort = self.current_reasoning_effort();
        let request = ClientRequest::TurnStart {
            request_id: self.next_request_id(),
            params: build_turn_start_params(thread_id, input, collaboration_mode, effort),
        };
        let response: TurnStartResponse = self.send_request(request, "turn/start").await?;
        *self.turn_id.lock().await = Some(response.turn.id.clone());
        Ok(response)
    }

    pub async fn send_direct_control(
        &self,
        control: DirectControl,
    ) -> Result<Vec<u8>, ExecutorError> {
        match control {
            DirectControl::Cancel => {
                let thread_id = self.thread_id.lock().await.clone().ok_or_else(|| {
                    ExecutorError::Io(io::Error::other("Codex has no active thread"))
                })?;
                let turn_id = self.turn_id.lock().await.clone().ok_or_else(|| {
                    ExecutorError::Io(io::Error::other("Codex has no active turn"))
                })?;
                let request = ClientRequest::TurnInterrupt {
                    request_id: self.next_request_id(),
                    params: TurnInterruptParams { thread_id, turn_id },
                };
                // Cancellation is authoritative at the host boundary.  The
                // interrupt frame only needs to be written successfully; the
                // process host kills the process immediately afterwards and
                // must not wait for an app-server acknowledgement.
                self.rpc().send_with_raw(&request).await
            }
            DirectControl::Steer { text } => {
                let thread_id = self.thread_id.lock().await.clone().ok_or_else(|| {
                    ExecutorError::Io(io::Error::other("Codex has no active thread"))
                })?;
                let expected_turn_id = self.turn_id.lock().await.clone().ok_or_else(|| {
                    ExecutorError::Io(io::Error::other("Codex has no active turn"))
                })?;
                let request = ClientRequest::TurnSteer {
                    request_id: self.next_request_id(),
                    params: TurnSteerParams {
                        thread_id,
                        client_user_message_id: None,
                        input: vec![UserInput::Text {
                            text,
                            text_elements: vec![],
                        }],
                        responsesapi_client_metadata: None,
                        additional_context: None,
                        expected_turn_id,
                    },
                };
                let (_, raw) = self
                    .rpc()
                    .request_with_raw::<TurnSteerResponse, _>(
                        request_id(&request),
                        &request,
                        "turn/steer",
                        self.cancel.clone(),
                    )
                    .await?;
                Ok(raw)
            }
            DirectControl::UpdatePlanGoalDraft { objective } => {
                let mut pending = self.pending_plan_goal_draft.lock().await;
                if self.execution_mode != ExecutionMode::PlanWithGoal || pending.is_none() {
                    return Err(ExecutorError::Io(io::Error::other(
                        "Codex has no Plan with Goal draft awaiting approval",
                    )));
                }
                *pending = Some(objective);
                serde_json::to_vec(&serde_json::json!({
                    "method": "easy-vibe/plan-goal-draft/updated"
                }))
                .map_err(ExecutorError::from)
            }
            DirectControl::GoalUpdate {
                objective,
                status,
                token_budget,
            } => {
                let thread_id = self.thread_id.lock().await.clone().ok_or_else(|| {
                    ExecutorError::Io(io::Error::other("Codex has no active thread"))
                })?;
                let status = status.map(|status| match status {
                    crate::runtime::AgentGoalStatus::Active => ThreadGoalStatus::Active,
                    crate::runtime::AgentGoalStatus::Paused => ThreadGoalStatus::Paused,
                    crate::runtime::AgentGoalStatus::Blocked => ThreadGoalStatus::Blocked,
                    crate::runtime::AgentGoalStatus::UsageLimited => ThreadGoalStatus::UsageLimited,
                    crate::runtime::AgentGoalStatus::BudgetLimited => {
                        ThreadGoalStatus::BudgetLimited
                    }
                    crate::runtime::AgentGoalStatus::Complete => ThreadGoalStatus::Complete,
                });
                let request = ClientRequest::ThreadGoalSet {
                    request_id: self.next_request_id(),
                    params: ThreadGoalSetParams {
                        thread_id,
                        objective,
                        status,
                        token_budget,
                    },
                };
                let (_, raw) = self
                    .rpc()
                    .request_with_raw::<ThreadGoalSetResponse, _>(
                        request_id(&request),
                        &request,
                        "thread/goal/set",
                        self.cancel.clone(),
                    )
                    .await?;
                Ok(raw)
            }
            DirectControl::GoalClear => {
                let thread_id = self.thread_id.lock().await.clone().ok_or_else(|| {
                    ExecutorError::Io(io::Error::other("Codex has no active thread"))
                })?;
                let request = ClientRequest::ThreadGoalClear {
                    request_id: self.next_request_id(),
                    params: ThreadGoalClearParams { thread_id },
                };
                let (_, raw) = self
                    .rpc()
                    .request_with_raw::<ThreadGoalClearResponse, _>(
                        request_id(&request),
                        &request,
                        "thread/goal/clear",
                        self.cancel.clone(),
                    )
                    .await?;
                Ok(raw)
            }
            DirectControl::Approve { .. } | DirectControl::Input { .. } => {
                Err(ExecutorError::Io(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Codex approval/input responses must use the original server request id",
                )))
            }
        }
    }

    fn collaboration_mode_with_reasoning(
        &self,
        mode: ModeKind,
        reasoning_effort: Option<ProtocolReasoningEffort>,
    ) -> Result<CollaborationMode, ExecutorError> {
        let model = self.resolved_model.get().cloned().ok_or_else(|| {
            tracing::error!(
                "collaboration_mode_with_reasoning called before resolved_model was set"
            );
            ExecutorError::Io(io::Error::other(
                "resolved model not available for collaboration mode",
            ))
        })?;
        Ok(CollaborationMode {
            mode,
            settings: Settings {
                model,
                reasoning_effort,
                developer_instructions: None,
            },
        })
    }

    fn collaboration_mode(&self, mode: ModeKind) -> Result<CollaborationMode, ExecutorError> {
        self.collaboration_mode_with_reasoning(mode, self.current_reasoning_effort())
    }

    pub fn initial_collaboration_mode(&self) -> Result<CollaborationMode, ExecutorError> {
        if self.plan_mode {
            self.collaboration_mode(ModeKind::Plan)
        } else {
            self.collaboration_mode(ModeKind::Default)
        }
    }

    fn current_reasoning_effort(&self) -> Option<ProtocolReasoningEffort> {
        match self.reasoning_effort.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::error!("Codex reasoning effort state was poisoned");
                poisoned.into_inner().clone()
            }
        }
    }

    fn set_reasoning_effort(&self, effort: Option<ProtocolReasoningEffort>) {
        match self.reasoning_effort.lock() {
            Ok(mut guard) => {
                *guard = effort;
            }
            Err(poisoned) => {
                tracing::error!("Codex reasoning effort state was poisoned");
                *poisoned.into_inner() = effort;
            }
        }
    }

    pub async fn get_account(&self) -> Result<GetAccountResponse, ExecutorError> {
        let request = ClientRequest::GetAccount {
            request_id: self.next_request_id(),
            params: GetAccountParams {
                refresh_token: false,
            },
        };
        self.send_request(request, "account/read").await
    }

    pub async fn start_review(
        &self,
        thread_id: String,
        target: ReviewTarget,
    ) -> Result<ReviewStartResponse, ExecutorError> {
        let request = ClientRequest::ReviewStart {
            request_id: self.next_request_id(),
            params: ReviewStartParams {
                thread_id,
                target,
                delivery: None,
            },
        };
        self.send_request(request, "reviewStart").await
    }

    pub async fn list_mcp_server_status(
        &self,
        cursor: Option<String>,
    ) -> Result<ListMcpServerStatusResponse, ExecutorError> {
        let thread_id = self.thread_id.lock().await.clone();
        let request = ClientRequest::McpServerStatusList {
            request_id: self.next_request_id(),
            params: ListMcpServerStatusParams {
                cursor,
                limit: None,
                detail: Some(McpServerStatusDetail::ToolsAndAuthOnly),
                thread_id,
            },
        };
        self.send_request(request, "mcpServerStatus/list").await
    }

    pub async fn skills_list(&self, cwd: PathBuf) -> Result<SkillsListResponse, ExecutorError> {
        let request = ClientRequest::SkillsList {
            request_id: self.next_request_id(),
            params: SkillsListParams {
                cwds: vec![cwd],
                force_reload: false,
            },
        };
        self.send_request(request, "skills/list").await
    }

    pub async fn model_list(&self) -> Result<ModelListResponse, ExecutorError> {
        let request = ClientRequest::ModelList {
            request_id: self.next_request_id(),
            params: ModelListParams::default(),
        };
        self.send_request(request, "model/list").await
    }

    pub async fn thread_compact_start(
        &self,
        thread_id: String,
    ) -> Result<ThreadCompactStartResponse, ExecutorError> {
        let request = ClientRequest::ThreadCompactStart {
            request_id: self.next_request_id(),
            params: ThreadCompactStartParams { thread_id },
        };
        self.send_request(request, "thread/compact/start").await
    }

    pub async fn thread_read(
        &self,
        thread_id: String,
    ) -> Result<ThreadReadResponse, ExecutorError> {
        let request = ClientRequest::ThreadRead {
            request_id: self.next_request_id(),
            params: ThreadReadParams {
                thread_id,
                include_turns: false,
            },
        };
        self.send_request(request, "thread/read").await
    }

    pub async fn thread_goal_set(
        &self,
        params: ThreadGoalSetParams,
    ) -> Result<ThreadGoalSetResponse, ExecutorError> {
        if let Some(objective) = params.objective.as_deref() {
            super::validate_goal_objective(objective)?;
        }
        let request = ClientRequest::ThreadGoalSet {
            request_id: self.next_request_id(),
            params,
        };
        self.send_request(request, "thread/goal/set").await
    }

    pub async fn start_goal(
        &self,
        thread_id: String,
        objective: String,
    ) -> Result<ThreadGoalSetResponse, ExecutorError> {
        self.activate_goal(ThreadGoalSetParams {
            thread_id,
            objective: Some(objective),
            status: Some(ThreadGoalStatus::Active),
            token_budget: Some(self.goal_token_budget),
        })
        .await
    }

    pub async fn thread_goal_get(
        &self,
        thread_id: String,
    ) -> Result<ThreadGoalGetResponse, ExecutorError> {
        let request = ClientRequest::ThreadGoalGet {
            request_id: self.next_request_id(),
            params: ThreadGoalGetParams { thread_id },
        };
        self.send_request(request, "thread/goal/get").await
    }

    pub async fn resume_goal(
        &self,
        thread_id: String,
    ) -> Result<ThreadGoalSetResponse, ExecutorError> {
        // The UI holds a historical snapshot; the provider owns the current goal.
        let current = self.thread_goal_get(thread_id.clone()).await?;
        let params = goal_resume_params(thread_id, current.goal.as_ref().map(|goal| &goal.status))?;
        self.activate_goal(params).await
    }

    async fn activate_goal(
        &self,
        params: ThreadGoalSetParams,
    ) -> Result<ThreadGoalSetResponse, ExecutorError> {
        if let Some(objective) = params.objective.as_deref() {
            super::validate_goal_objective(objective)?;
        }
        let id = self.next_request_id();
        self.goal_lifecycle
            .lock()
            .unwrap()
            .begin(id.clone(), params.thread_id.clone());
        let result = self
            .send_request(
                ClientRequest::ThreadGoalSet {
                    request_id: id.clone(),
                    params,
                },
                "thread/goal/set",
            )
            .await;
        if result.is_err() {
            self.goal_lifecycle.lock().unwrap().fail(&id);
        }
        result
    }

    pub async fn thread_goal_clear(
        &self,
        thread_id: String,
    ) -> Result<ThreadGoalClearResponse, ExecutorError> {
        let request = ClientRequest::ThreadGoalClear {
            request_id: self.next_request_id(),
            params: ThreadGoalClearParams { thread_id },
        };
        self.send_request(request, "thread/goal/clear").await
    }

    pub async fn thread_settings_update(
        &self,
        params: ThreadSettingsUpdateParams,
    ) -> Result<ThreadSettingsUpdateResponse, ExecutorError> {
        let reasoning_update = reasoning_update_from_thread_settings(&params);
        let request = ClientRequest::ThreadSettingsUpdate {
            request_id: self.next_request_id(),
            params,
        };
        let response = self.send_request(request, "thread/settings/update").await?;
        if let ReasoningEffortUpdate::Set(effort) = reasoning_update {
            self.set_reasoning_effort(effort);
        }
        Ok(response)
    }

    pub fn build_reasoning_thread_settings_update_params(
        &self,
        thread_id: String,
        effort: Option<ProtocolReasoningEffort>,
    ) -> Result<ThreadSettingsUpdateParams, ExecutorError> {
        let mode = if self.plan_mode {
            ModeKind::Plan
        } else {
            ModeKind::Default
        };
        let collaboration_mode = self.collaboration_mode_with_reasoning(mode, effort.clone())?;
        Ok(build_thread_settings_update_params(
            thread_id,
            effort,
            Some(collaboration_mode),
        ))
    }

    pub async fn config_batch_write(
        &self,
        edits: Vec<ConfigEdit>,
    ) -> Result<ConfigWriteResponse, ExecutorError> {
        let request = ClientRequest::ConfigBatchWrite {
            request_id: self.next_request_id(),
            params: ConfigBatchWriteParams {
                edits,
                file_path: None,
                expected_version: None,
                reload_user_config: false,
            },
        };
        self.send_request(request, "config/batchWrite").await
    }

    pub async fn config_read(
        &self,
        cwd: Option<String>,
    ) -> Result<ConfigReadResponse, ExecutorError> {
        let request = ClientRequest::ConfigRead {
            request_id: self.next_request_id(),
            params: ConfigReadParams {
                include_layers: false,
                cwd,
            },
        };
        self.send_request(request, "config/read").await
    }

    pub async fn get_account_rate_limits(
        &self,
    ) -> Result<GetAccountRateLimitsResponse, ExecutorError> {
        let request = ClientRequest::GetAccountRateLimits {
            request_id: self.next_request_id(),
            params: None,
        };
        self.send_request(request, "account/rateLimits/read").await
    }

    async fn handle_server_request(
        &self,
        peer: &JsonRpcPeer,
        request: ServerRequest,
    ) -> Result<(), ExecutorError> {
        match request {
            ServerRequest::FileChangeRequestApproval { request_id, params } => {
                let call_id = params.item_id.clone();
                let status = self
                    .request_tool_approval("edit", "codex.apply_patch", &call_id)
                    .await
                    .inspect_err(|err| {
                        if !matches!(
                            err,
                            ExecutorError::ExecutorApprovalError(ExecutorApprovalError::Cancelled)
                        ) {
                            tracing::error!(
                                "Codex file_change approval failed for item_id={}: {err}",
                                call_id
                            );
                        }
                    })?;
                self.log_writer
                    .log_raw(
                        &Approval::approval_response(
                            call_id,
                            "codex.apply_patch".to_string(),
                            status.clone(),
                        )
                        .raw(),
                    )
                    .await?;
                let (decision, feedback) = self.file_change_decision(&status);
                let response = FileChangeRequestApprovalResponse { decision };
                send_server_response(peer, request_id, response).await?;
                if let Some(message) = feedback {
                    tracing::debug!("queueing file change denial feedback: {message}");
                    self.enqueue_feedback(message).await;
                }
                Ok(())
            }
            ServerRequest::CommandExecutionRequestApproval { request_id, params } => {
                let call_id = params.item_id.clone();
                let status = self
                    .request_tool_approval("bash", "codex.exec_command", &call_id)
                    .await
                    .inspect_err(|err| {
                        if !matches!(
                            err,
                            ExecutorError::ExecutorApprovalError(ExecutorApprovalError::Cancelled)
                        ) {
                            tracing::error!(
                                "Codex command_execution approval failed for item_id={}: {err}",
                                call_id
                            );
                        }
                    })?;
                self.log_writer
                    .log_raw(
                        &Approval::approval_response(
                            call_id,
                            "codex.exec_command".to_string(),
                            status.clone(),
                        )
                        .raw(),
                    )
                    .await?;
                let (decision, feedback) = self.command_execution_decision(&status);
                let response = CommandExecutionRequestApprovalResponse { decision };
                send_server_response(peer, request_id, response).await?;
                if let Some(message) = feedback {
                    tracing::debug!("queueing exec denial feedback: {message}");
                    self.enqueue_feedback(message).await;
                }
                Ok(())
            }
            ServerRequest::ToolRequestUserInput { request_id, params } => {
                let call_id = params.item_id.clone();
                let question_count = params.questions.len();
                let status = self
                    .request_question_answer(
                        question_count,
                        &call_id,
                        params.auto_resolution_ms.map(Duration::from_millis),
                    )
                    .await
                    .inspect_err(|err| {
                        if !matches!(
                            err,
                            ExecutorError::ExecutorApprovalError(ExecutorApprovalError::Cancelled)
                        ) {
                            tracing::error!(
                                "Codex question approval failed for call_id={}: {err}",
                                call_id
                            );
                        }
                    })?;
                self.log_writer
                    .log_raw(&Approval::question_response(call_id.clone(), status.clone()).raw())
                    .await?;
                let response = match &status {
                    QuestionStatus::Answered { answers } => {
                        let answers_map: HashMap<String, Vec<String>> = answers
                            .iter()
                            .map(|qa| (qa.question.clone(), qa.answer.clone()))
                            .collect();
                        answers_to_codex_format(&params.questions, &answers_map)
                    }
                    _ => ToolRequestUserInputResponse {
                        answers: HashMap::new(),
                    },
                };
                send_server_response(peer, request_id, response).await?;
                Ok(())
            }
            ServerRequest::CurrentTimeRead { request_id, .. } => {
                send_server_response(
                    peer,
                    request_id,
                    CurrentTimeReadResponse {
                        current_time_at: chrono::Utc::now().timestamp(),
                    },
                )
                .await
            }
            ServerRequest::McpServerElicitationRequest { request_id, params } => {
                tracing::warn!(
                    server = %params.server_name,
                    "MCP elicitation UI is unavailable; cancelling the request"
                );
                send_server_response(
                    peer,
                    request_id,
                    McpServerElicitationRequestResponse {
                        action: McpServerElicitationAction::Cancel,
                        content: None,
                        meta: None,
                    },
                )
                .await
            }
            ServerRequest::DynamicToolCall { request_id, params } => {
                tracing::warn!(
                    "received unsupported dynamic tool call: tool={} call_id={}",
                    params.tool,
                    params.call_id
                );
                let response = DynamicToolCallResponse {
                    content_items: vec![DynamicToolCallOutputContentItem::InputText {
                        text: format!(
                            "Dynamic tool '{}' is not supported by this client.",
                            params.tool
                        ),
                    }],
                    success: false,
                };
                send_server_response(peer, request_id, response).await?;
                Ok(())
            }
            ServerRequest::ChatgptAuthTokensRefresh { .. }
            | ServerRequest::AttestationGenerate { .. }
            | ServerRequest::PermissionsRequestApproval { .. } => {
                tracing::warn!("received unhandled v2 server request: {:?}", request);
                let response = JSONRPCResponse {
                    id: request.id().clone(),
                    result: Value::Null,
                };
                peer.send(&response).await
            }
            ServerRequest::ApplyPatchApproval { .. }
            | ServerRequest::ExecCommandApproval { .. } => {
                tracing::error!(
                    "received deprecated v1 server request (session may have been started with legacy API): {:?}",
                    request
                );
                Err(ExecutorApprovalError::RequestFailed(
                    "deprecated v1 server request".to_string(),
                )
                .into())
            }
        }
    }

    async fn request_tool_approval(
        &self,
        tool_name: &str,
        display_tool_name: &str,
        tool_call_id: &str,
    ) -> Result<ApprovalStatus, ExecutorError> {
        if self.auto_approve {
            return Ok(ApprovalStatus::Approved);
        }
        let approval_service = self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)?;

        let approval_id = approval_service
            .create_tool_approval(tool_name)
            .or_else(|err| async {
                self.handle_approval_error(display_tool_name, tool_call_id)
                    .await;
                Err(err)
            })
            .await?;

        let _ = self
            .log_writer
            .log_raw(
                &Approval::approval_requested(
                    tool_call_id.to_string(),
                    display_tool_name.to_string(),
                    approval_id.clone(),
                )
                .raw(),
            )
            .await;

        approval_service
            .wait_tool_approval(&approval_id, self.cancel.clone())
            .or_else(|err| async {
                self.handle_approval_error(display_tool_name, tool_call_id)
                    .await;
                Err(err)
            })
            .await
            .map_err(ExecutorError::from)
    }

    async fn handle_approval_error(&self, display_tool_name: &str, tool_call_id: &str) {
        let _ = self
            .log_writer
            .log_raw(
                &Approval::approval_response(
                    tool_call_id.to_string(),
                    display_tool_name.to_string(),
                    ApprovalStatus::TimedOut,
                )
                .raw(),
            )
            .await;
    }

    async fn request_question_answer(
        &self,
        question_count: usize,
        tool_call_id: &str,
        timeout: Option<Duration>,
    ) -> Result<QuestionStatus, ExecutorError> {
        let approval_service = self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)?;

        let approval_id = approval_service
            .create_question_approval_with_timeout("question", question_count, timeout)
            .or_else(|err| async {
                self.handle_question_error(tool_call_id).await;
                Err(err)
            })
            .await?;

        let _ = self
            .log_writer
            .log_raw(
                &Approval::approval_requested(
                    tool_call_id.to_string(),
                    "codex.question".to_string(),
                    approval_id.clone(),
                )
                .raw(),
            )
            .await;

        approval_service
            .wait_question_answer(&approval_id, self.cancel.clone())
            .or_else(|err| async {
                self.handle_question_error(tool_call_id).await;
                Err(err)
            })
            .await
            .map_err(ExecutorError::from)
    }

    async fn handle_question_error(&self, tool_call_id: &str) {
        let _ = self
            .log_writer
            .log_raw(
                &Approval::question_response(tool_call_id.to_string(), QuestionStatus::TimedOut)
                    .raw(),
            )
            .await;
    }

    async fn handle_plan_completed(&self, plan: PendingPlan) -> Result<bool, ExecutorError> {
        let approval_service = self
            .approvals
            .as_ref()
            .ok_or(ExecutorApprovalError::ServiceUnavailable)?;

        if self.execution_mode == ExecutionMode::PlanWithGoal {
            *self.pending_plan_goal_draft.lock().await = Some(plan.text.clone());
        }

        let approval_id = approval_service
            .create_tool_approval("plan")
            .or_else(|err| async {
                self.handle_approval_error("codex.plan", &plan.item_id)
                    .await;
                Err(err)
            })
            .await?;

        let _ = self
            .log_writer
            .log_raw(
                &Approval::approval_requested(
                    plan.item_id.clone(),
                    "codex.plan".to_string(),
                    approval_id.clone(),
                )
                .raw(),
            )
            .await;

        let status = approval_service
            .wait_tool_approval(&approval_id, self.cancel.clone())
            .or_else(|err| async {
                self.handle_approval_error("codex.plan", &plan.item_id)
                    .await;
                Err(err)
            })
            .await
            .map_err(ExecutorError::from)?;

        self.log_writer
            .log_raw(
                &Approval::approval_response(
                    plan.item_id,
                    "codex.plan".to_string(),
                    status.clone(),
                )
                .raw(),
            )
            .await?;

        let Some(thread_id) = self.thread_id.lock().await.clone() else {
            return Ok(true);
        };

        match status {
            ApprovalStatus::Approved => {
                if self.execution_mode == ExecutionMode::PlanWithGoal {
                    let objective = self
                        .pending_plan_goal_draft
                        .lock()
                        .await
                        .take()
                        .unwrap_or(plan.text);
                    self.start_goal(thread_id, objective).await?;
                    return Ok(false);
                }
                self.spawn_turn_start(
                    thread_id,
                    "Implement the plan.".to_string(),
                    Some(self.collaboration_mode(ModeKind::Default)?),
                );
                Ok(false)
            }
            ApprovalStatus::Denied { reason } => {
                *self.pending_plan_goal_draft.lock().await = None;
                let feedback = reason
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                if let Some(feedback_text) = feedback {
                    self.spawn_turn_start(
                        thread_id,
                        format!("User feedback on the plan: {feedback_text}"),
                        Some(self.collaboration_mode(ModeKind::Plan)?),
                    );
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
            ApprovalStatus::TimedOut | ApprovalStatus::Pending => {
                *self.pending_plan_goal_draft.lock().await = None;
                Ok(true)
            }
        }
    }

    pub async fn register_session(&self, thread_id: &str) -> Result<(), ExecutorError> {
        {
            let mut guard = self.thread_id.lock().await;
            guard.replace(thread_id.to_string());
        }
        self.flush_pending_feedback().await;
        Ok(())
    }

    async fn send_message<M>(&self, message: &M) -> Result<(), ExecutorError>
    where
        M: Serialize + Sync,
    {
        self.rpc().send(message).await
    }

    async fn send_request<R>(&self, request: ClientRequest, label: &str) -> Result<R, ExecutorError>
    where
        R: DeserializeOwned + std::fmt::Debug,
    {
        let request_id = request_id(&request);
        self.rpc()
            .request(request_id, &request, label, self.cancel.clone())
            .await
    }

    fn next_request_id(&self) -> RequestId {
        self.rpc().next_request_id()
    }

    fn command_execution_decision(
        &self,
        status: &ApprovalStatus,
    ) -> (CommandExecutionApprovalDecision, Option<String>) {
        if self.auto_approve {
            return (CommandExecutionApprovalDecision::AcceptForSession, None);
        }

        match status {
            ApprovalStatus::Approved => (CommandExecutionApprovalDecision::Accept, None),
            ApprovalStatus::Denied { reason } => {
                let feedback = reason
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                if feedback.is_some() {
                    (CommandExecutionApprovalDecision::Cancel, feedback)
                } else {
                    (CommandExecutionApprovalDecision::Decline, None)
                }
            }
            ApprovalStatus::TimedOut => (CommandExecutionApprovalDecision::Decline, None),
            ApprovalStatus::Pending => (CommandExecutionApprovalDecision::Decline, None),
        }
    }

    fn file_change_decision(
        &self,
        status: &ApprovalStatus,
    ) -> (FileChangeApprovalDecision, Option<String>) {
        if self.auto_approve {
            return (FileChangeApprovalDecision::AcceptForSession, None);
        }

        match status {
            ApprovalStatus::Approved => (FileChangeApprovalDecision::Accept, None),
            ApprovalStatus::Denied { reason } => {
                let feedback = reason
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());
                if feedback.is_some() {
                    (FileChangeApprovalDecision::Cancel, feedback)
                } else {
                    (FileChangeApprovalDecision::Decline, None)
                }
            }
            ApprovalStatus::TimedOut => (FileChangeApprovalDecision::Decline, None),
            ApprovalStatus::Pending => (FileChangeApprovalDecision::Decline, None),
        }
    }

    async fn enqueue_feedback(&self, message: String) {
        if message.trim().is_empty() {
            return;
        }
        let mut guard = self.pending_feedback.lock().await;
        guard.push_back(message);
    }

    /// Sends pending feedback messages as new turns.
    /// Returns `true` if any messages were sent.
    async fn flush_pending_feedback(&self) -> bool {
        let messages: Vec<String> = {
            let mut guard = self.pending_feedback.lock().await;
            guard.drain(..).collect()
        };

        if messages.is_empty() {
            return false;
        }

        let Some(thread_id) = self.thread_id.lock().await.clone() else {
            tracing::warn!(
                "pending Codex feedback but thread id unavailable; dropping {} messages",
                messages.len()
            );
            return false;
        };

        let mut sent = false;
        for message in messages {
            let trimmed = message.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.spawn_user_message(thread_id.clone(), format!("User feedback: {trimmed}"));
            sent = true;
        }
        sent
    }

    fn spawn_turn_start(
        &self,
        thread_id: String,
        message: String,
        collaboration_mode: Option<CollaborationMode>,
    ) {
        let peer = self.rpc().clone();
        let cancel = self.cancel.clone();
        let turn_id = self.turn_id.clone();
        let effort = self.current_reasoning_effort();
        let request = ClientRequest::TurnStart {
            request_id: peer.next_request_id(),
            params: build_turn_start_params(
                thread_id,
                vec![UserInput::Text {
                    text: message,
                    text_elements: vec![],
                }],
                collaboration_mode,
                effort,
            ),
        };
        tokio::spawn(async move {
            match peer
                .request::<TurnStartResponse, _>(
                    request_id(&request),
                    &request,
                    "turn/start",
                    cancel,
                )
                .await
            {
                Ok(response) => {
                    *turn_id.lock().await = Some(response.turn.id);
                }
                Err(err) => tracing::error!("failed to send user message: {err}"),
            }
        });
    }

    fn spawn_user_message(&self, thread_id: String, message: String) {
        self.spawn_turn_start(thread_id, message, None);
    }
}

pub(crate) fn build_turn_start_params(
    thread_id: String,
    input: Vec<UserInput>,
    collaboration_mode: Option<CollaborationMode>,
    effort: Option<ProtocolReasoningEffort>,
) -> TurnStartParams {
    TurnStartParams {
        thread_id,
        input,
        effort,
        collaboration_mode,
        ..Default::default()
    }
}

fn goal_resume_params(
    thread_id: String,
    status: Option<&ThreadGoalStatus>,
) -> Result<ThreadGoalSetParams, ExecutorError> {
    match status {
        None => {
            return Err(ExecutorError::Io(std::io::Error::other(
                "No saved Goal exists in this Codex session. Create a new Goal explicitly.",
            )));
        }
        Some(ThreadGoalStatus::Complete) => {
            return Err(ExecutorError::Io(std::io::Error::other(
                "This Codex Goal is already complete. Create a new Goal explicitly.",
            )));
        }
        _ => {}
    }
    Ok(ThreadGoalSetParams {
        thread_id,
        objective: None,
        status: Some(ThreadGoalStatus::Active),
        // Omitted, not Some(None): keep the provider's budget and accounting.
        token_budget: None,
    })
}

fn build_thread_settings_update_params(
    thread_id: String,
    effort: Option<ProtocolReasoningEffort>,
    collaboration_mode: Option<CollaborationMode>,
) -> ThreadSettingsUpdateParams {
    ThreadSettingsUpdateParams {
        thread_id,
        effort,
        collaboration_mode,
        ..Default::default()
    }
}

enum ReasoningEffortUpdate {
    Unchanged,
    Set(Option<ProtocolReasoningEffort>),
}

fn reasoning_update_from_thread_settings(
    params: &ThreadSettingsUpdateParams,
) -> ReasoningEffortUpdate {
    if let Some(collaboration_mode) = &params.collaboration_mode {
        return ReasoningEffortUpdate::Set(collaboration_mode.settings.reasoning_effort.clone());
    }
    if let Some(effort) = &params.effort {
        return ReasoningEffortUpdate::Set(Some(effort.clone()));
    }
    ReasoningEffortUpdate::Unchanged
}

fn turn_completion_control_flow(status: &TurnStatus, keep_alive: bool) -> JsonRpcControlFlow {
    if keep_alive && matches!(status, TurnStatus::Completed | TurnStatus::Interrupted) {
        return JsonRpcControlFlow::Continue;
    }

    match status {
        TurnStatus::Completed => JsonRpcControlFlow::Exit(ExecutorExitResult::Success),
        TurnStatus::Interrupted | TurnStatus::Failed | TurnStatus::InProgress => {
            JsonRpcControlFlow::Exit(ExecutorExitResult::Failure)
        }
    }
}

fn clear_completed_turn(active_turn_id: &mut Option<String>, completed_turn_id: &str) {
    if active_turn_id.as_deref() == Some(completed_turn_id) {
        *active_turn_id = None;
    }
}

fn parse_turn_completed_params(
    params: Option<&Value>,
) -> Result<CompletedTurnNotification, ExecutorError> {
    let params = params.ok_or_else(|| {
        ExecutorError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "turn/completed notification missing params",
        ))
    })?;
    serde_json::from_value(params.clone()).map_err(ExecutorError::from)
}

#[async_trait]
impl ExecutorControl for AppServerClient {
    async fn send(&self, control: DirectControl) -> Result<Vec<u8>, ExecutorError> {
        self.send_direct_control(control).await
    }
}

#[async_trait]
impl JsonRpcCallbacks for AppServerClient {
    async fn on_request(
        &self,
        peer: &JsonRpcPeer,
        raw: &str,
        request: JSONRPCRequest,
    ) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await?;
        match ServerRequest::try_from(request.clone()) {
            Ok(server_request) => {
                let client = self.self_ref.upgrade().expect("live client");
                let peer = peer.clone();
                tokio::spawn(async move {
                    if let Err(error) = client.handle_server_request(&peer, server_request).await {
                        let _ = client
                            .log_writer
                            .log_raw(&format!("Codex server request failed: {error}"))
                            .await;
                        peer.request_exit(ExecutorExitResult::Failure);
                    }
                });
                Ok(())
            }
            Err(err) => {
                tracing::debug!("Unhandled server request `{}`: {err}", request.method);
                let response = JSONRPCResponse {
                    id: request.id,
                    result: Value::Null,
                };
                peer.send(&response).await
            }
        }
    }

    async fn on_response(
        &self,
        _peer: &JsonRpcPeer,
        raw: &str,
        response: &JSONRPCResponse,
    ) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await?;
        if let Some(thread) = response
            .result
            .pointer("/thread/id")
            .and_then(Value::as_str)
        {
            let mut target = self.thread_id.lock().await;
            if target.is_none() {
                *target = Some(thread.to_owned());
            }
        }
        // Fence activation in wire order, before the RPC future is resolved.
        let mut state = self.goal_lifecycle.lock().unwrap();
        if state.awaiting(&response.id) {
            let goal: ThreadGoalSetResponse = serde_json::from_value(response.result.clone())?;
            let status = if goal.goal.status == ThreadGoalStatus::Active {
                "active"
            } else {
                "invalid"
            };
            state
                .acknowledge(&response.id, &goal.goal.thread_id, status)
                .map_err(|message| ExecutorError::Io(io::Error::other(message)))?;
        }
        Ok(())
    }

    async fn on_error(
        &self,
        _peer: &JsonRpcPeer,
        raw: &str,
        error: &JSONRPCError,
    ) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await?;
        let mut state = self.goal_lifecycle.lock().unwrap();
        if state.awaiting(&error.id) {
            state.fail(&error.id);
            return Err(ExecutorError::Io(io::Error::other(format!(
                "Goal activation failed: {}",
                error.error.message
            ))));
        }
        Ok(())
    }

    async fn on_notification(
        &self,
        _peer: &JsonRpcPeer,
        raw: &str,
        notification: JSONRPCNotification,
    ) -> Result<JsonRpcControlFlow, ExecutorError> {
        self.process_notification(raw, notification).await
    }

    async fn on_non_json(&self, raw: &str) -> Result<(), ExecutorError> {
        self.log_writer.log_raw(raw).await?;
        Ok(())
    }
}

impl AppServerClient {
    async fn process_notification(
        &self,
        raw: &str,
        notification: JSONRPCNotification,
    ) -> Result<JsonRpcControlFlow, ExecutorError> {
        self.log_writer.log_raw(raw).await?;

        let method = notification.method.as_str();
        let notification_thread = notification
            .params
            .as_ref()
            .and_then(|params| params.get("threadId"))
            .and_then(Value::as_str);
        if method.starts_with("thread/goal/")
            || method.starts_with("turn/")
            || method.starts_with("item/")
        {
            let target = self.thread_id.lock().await;
            if notification_thread.is_none() || notification_thread != target.as_deref() {
                return Ok(JsonRpcControlFlow::Continue);
            }
        }

        // Detect completed plan items in the notification stream
        if self.plan_mode
            && method == "item/completed"
            && let Some(ref params) = notification.params
            && let Ok(completed) =
                serde_json::from_value::<ItemCompletedNotification>(params.clone())
            && let ThreadItem::Plan { id, text } = completed.item
        {
            *self.pending_plan.lock().await = Some(PendingPlan { item_id: id, text });
        }

        if method == "turn/started"
            && let Some(turn_id) = notification
                .params
                .as_ref()
                .and_then(|params| params.pointer("/turn/id"))
                .and_then(Value::as_str)
        {
            *self.turn_id.lock().await = Some(turn_id.to_string());
        }

        if method == "thread/goal/updated"
            && let Some(status) = notification
                .params
                .as_ref()
                .and_then(|params| params.pointer("/goal/status"))
                .and_then(Value::as_str)
            && let Some(result) = self.observe_goal_updated(notification_thread.unwrap(), status)
        {
            // Goal persistence completes before the calling tool and final answer.
            // Dropping the process here would leave that turn interrupted in Codex.
            return Ok(if self.turn_id.lock().await.is_some() {
                JsonRpcControlFlow::Continue
            } else {
                JsonRpcControlFlow::Exit(result)
            });
        }

        if method == "thread/goal/cleared"
            && let Some(result) = self.observe_goal_updated(notification_thread.unwrap(), "cleared")
        {
            return Ok(if self.turn_id.lock().await.is_some() {
                JsonRpcControlFlow::Continue
            } else {
                JsonRpcControlFlow::Exit(result)
            });
        }

        // V2 turn completion detection
        if method == "turn/completed" {
            let completed = parse_turn_completed_params(notification.params.as_ref())?;
            {
                let mut turn_id = self.turn_id.lock().await;
                // Replayed/duplicate completions cannot finish an approval or
                // activation transition when no current turn is registered.
                if turn_id.as_deref() != Some(completed.turn.id.as_str()) {
                    return Ok(JsonRpcControlFlow::Continue);
                }
                clear_completed_turn(&mut turn_id, &completed.turn.id);
            }

            match completed.turn.status {
                TurnStatus::Failed => {
                    tracing::error!(
                        turn_id = %completed.turn.id,
                        error = ?completed.turn.error,
                        "Codex turn failed"
                    );
                    return Ok(turn_completion_control_flow(&TurnStatus::Failed, false));
                }
                TurnStatus::InProgress => {
                    tracing::warn!(
                        turn_id = %completed.turn.id,
                        "Codex sent turn/completed with an in-progress turn"
                    );
                    return Ok(turn_completion_control_flow(&TurnStatus::InProgress, false));
                }
                TurnStatus::Interrupted => {
                    tracing::debug!("Codex turn interrupted; flushing feedback queue");
                    let keep_alive =
                        self.flush_pending_feedback().await || self.goal_keeps_run_alive();
                    return Ok(turn_completion_control_flow(
                        &TurnStatus::Interrupted,
                        keep_alive,
                    ));
                }
                TurnStatus::Completed => {}
            }

            // Handle plan approval on turn completion
            let pending = if self.plan_mode {
                self.pending_plan.lock().await.take()
            } else {
                None
            };
            if let Some(plan) = pending {
                let client = self.self_ref.upgrade().expect("live client");
                tokio::spawn(async move {
                    match client.handle_plan_completed(plan).await {
                        Ok(false) => {}
                        Ok(true) => client.rpc().request_exit(ExecutorExitResult::Success),
                        Err(error) => {
                            let _ = client
                                .log_writer
                                .log_raw(&format!("Plan transition failed: {error}"))
                                .await;
                            client.rpc().request_exit(ExecutorExitResult::Failure);
                        }
                    }
                });
                return Ok(JsonRpcControlFlow::Continue);
            }

            if self.goal_keeps_run_alive() {
                return Ok(JsonRpcControlFlow::Continue);
            }

            // Handle commit reminder on turn completion
            if self.commit_reminder
                && !self.commit_reminder_sent.swap(true, Ordering::SeqCst)
                && let status = self.repo_context.check_uncommitted_changes().await
                && !status.is_empty()
                && let Some(thread_id) = self.thread_id.lock().await.clone()
            {
                let prompt = format!("{}\n{}", self.commit_reminder_prompt, status);
                self.spawn_user_message(thread_id, prompt);
                return Ok(JsonRpcControlFlow::Continue);
            }

            return Ok(turn_completion_control_flow(&TurnStatus::Completed, false));
        }

        Ok(JsonRpcControlFlow::Continue)
    }
}

async fn send_server_response<T>(
    peer: &JsonRpcPeer,
    request_id: RequestId,
    response: T,
) -> Result<(), ExecutorError>
where
    T: Serialize,
{
    let payload = JSONRPCResponse {
        id: request_id,
        result: serde_json::to_value(response)
            .map_err(|err| ExecutorError::Io(io::Error::other(err.to_string())))?,
    };

    peer.send(&payload).await
}

/// Convert our `HashMap<question_text, Vec<answer_labels>>` answer format to
/// Codex's `HashMap<question_id, ToolRequestUserInputAnswer>` format.
fn answers_to_codex_format(
    questions: &[ToolRequestUserInputQuestion],
    answers: &HashMap<String, Vec<String>>,
) -> ToolRequestUserInputResponse {
    let codex_answers = questions
        .iter()
        .filter_map(|q| {
            answers.get(&q.question).map(|answer_vec| {
                (
                    q.id.clone(),
                    ToolRequestUserInputAnswer {
                        answers: answer_vec.clone(),
                    },
                )
            })
        })
        .collect();

    ToolRequestUserInputResponse {
        answers: codex_answers,
    }
}

fn request_id(request: &ClientRequest) -> RequestId {
    match request {
        ClientRequest::Initialize { request_id, .. }
        | ClientRequest::ThreadStart { request_id, .. }
        | ClientRequest::ThreadResume { request_id, .. }
        | ClientRequest::ThreadFork { request_id, .. }
        | ClientRequest::TurnStart { request_id, .. }
        | ClientRequest::GetAccount { request_id, .. }
        | ClientRequest::ReviewStart { request_id, .. }
        | ClientRequest::McpServerStatusList { request_id, .. }
        | ClientRequest::SkillsList { request_id, .. }
        | ClientRequest::ModelList { request_id, .. }
        | ClientRequest::ThreadCompactStart { request_id, .. }
        | ClientRequest::ThreadRead { request_id, .. }
        | ClientRequest::ThreadGoalSet { request_id, .. }
        | ClientRequest::ThreadGoalGet { request_id, .. }
        | ClientRequest::ThreadGoalClear { request_id, .. }
        | ClientRequest::ThreadSettingsUpdate { request_id, .. }
        | ClientRequest::ConfigRead { request_id, .. }
        | ClientRequest::ConfigBatchWrite { request_id, .. }
        | ClientRequest::GetAccountRateLimits { request_id, .. } => request_id.clone(),
        _ => unreachable!("request_id called for unsupported request variant"),
    }
}

fn ensure_resumed_thread_id(requested: &str, actual: &str) -> Result<(), ExecutorError> {
    if requested == actual {
        return Ok(());
    }

    Err(ExecutorError::Io(io::Error::other(format!(
        "Codex thread/resume returned a different thread id: requested {requested}, got {actual}"
    ))))
}

#[derive(Clone)]
pub struct LogWriter {
    writer: Arc<Mutex<BufWriter<Box<dyn AsyncWrite + Send + Unpin>>>>,
}

impl LogWriter {
    pub fn new(writer: impl AsyncWrite + Send + Unpin + 'static) -> Self {
        Self {
            writer: Arc::new(Mutex::new(BufWriter::new(Box::new(writer)))),
        }
    }

    pub async fn log_raw(&self, raw: &str) -> Result<(), ExecutorError> {
        let mut guard = self.writer.lock().await;
        guard
            .write_all(raw.as_bytes())
            .await
            .map_err(ExecutorError::Io)?;
        guard.write_all(b"\n").await.map_err(ExecutorError::Io)?;
        guard.flush().await.map_err(ExecutorError::Io)?;
        Ok(())
    }
}

/// Record the runtime version without turning it into a hard compatibility
/// requirement. The protocol crate used to compile this adapter is a build
/// dependency; it must not force users to install that exact CLI release.
fn observe_codex_version(user_agent: &str) {
    match extract_semver(user_agent) {
        Some(version) => tracing::debug!(codex_version = %version, "connected to Codex app-server"),
        None => tracing::debug!(
            user_agent = %user_agent,
            "connected to Codex app-server without a parsed version"
        ),
    }
}

/// Extracts the first `x.y.z`-shaped token from a user-agent string such as
/// `codex/0.144.1 (Windows 10; x86_64) vibe-codex-executor`.
fn extract_semver(input: &str) -> Option<String> {
    input
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|token| !token.is_empty())
        .find(|token| {
            token.matches('.').count() >= 2
                && token.chars().next().is_some_and(|c| c.is_ascii_digit())
                && token.chars().last().is_some_and(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

#[cfg(test)]
mod version_check_tests {
    // Run the test executable itself as a deterministic stdio provider. No Codex
    // account, model calls, shell, Python, or timing-based sleeps are required.
    #[test]
    #[ignore = "subprocess fixture, invoked by transport tests"]
    fn goal_stdio_fixture() {
        use std::io::{BufRead, Write};
        let case = std::env::var("EVK_GOAL_FIXTURE").expect("fixture case");
        let emit = |value: serde_json::Value| {
            println!("{value}");
            std::io::stdout().flush().unwrap();
        };
        let goal = |status: &str| {
            json!({"threadId":"thread-1", "objective":"existing objective", "status":status,
            "tokenBudget":1000,"tokensUsed":100,"timeUsedSeconds":12,"createdAt":1,"updatedAt":2})
        };
        let event = |method: &str, params: serde_json::Value| {
            emit(json!({"method":method,"params":params}))
        };
        for line in std::io::stdin().lock().lines() {
            let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
            match request["method"].as_str().unwrap() {
                "fixture/go" if case == "plan" => {
                    event(
                        "turn/started",
                        json!({"threadId":"thread-1","turn":{"id":"plan-turn"}}),
                    );
                    event(
                        "item/completed",
                        json!({"threadId":"thread-1","turnId":"plan-turn","completedAtMs":1,"item":{"type":"plan","id":"plan-item","text":"approved objective"}}),
                    );
                    event(
                        "turn/completed",
                        json!({"threadId":"thread-1","turn":{"id":"plan-turn","items":[],"status":"completed","error":null}}),
                    );
                }
                "fixture/go" => {}
                "thread/goal/get" => {
                    emit(
                        json!({"id":request["id"],"result":{"goal": if case == "missing" { serde_json::Value::Null } else { goal("paused") }}}),
                    );
                }
                "thread/goal/set" => {
                    // Snapshot from resume delivered after activation was sent.
                    event("thread/goal/cleared", json!({"threadId":"thread-1"}));
                    event(
                        "thread/goal/updated",
                        json!({"threadId":"thread-1","goal":goal("complete")}),
                    );
                    if case == "reject" {
                        emit(
                            json!({"id":request["id"],"error":{"code":-32602,"message":"activation rejected"}}),
                        );
                        continue;
                    }
                    if case == "resume" {
                        assert!(request["params"]["objective"].is_null());
                        assert!(request["params"].get("tokenBudget").is_none());
                    }
                    if case == "cancel" {
                        continue;
                    }
                    emit(json!({"id":request["id"],"result":{"goal":goal("active")}}));
                    if case == "eof" {
                        return;
                    }
                    // Child-thread notifications must not terminate the parent.
                    event("thread/goal/cleared", json!({"threadId":"child"}));
                    event(
                        "turn/started",
                        json!({"threadId":"thread-1","turn":{"id":"goal-turn"}}),
                    );
                    event(
                        "thread/goal/updated",
                        json!({"threadId":"thread-1","goal":goal("complete")}),
                    );
                    event(
                        "item/completed",
                        json!({"threadId":"thread-1","item":{"type":"agentMessage","text":"FINAL ANSWER AFTER GOAL COMPLETE"}}),
                    );
                    event(
                        "turn/completed",
                        json!({"threadId":"thread-1","turn":{"id":"goal-turn","items":[],"status":if case == "turn-failed" {"failed"} else {"completed"},"error":null}}),
                    );
                }
                other => panic!("unexpected method: {other}"),
            }
        }
    }

    #[derive(Clone)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl tokio::io::AsyncWrite for Capture {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            data: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            self.0.lock().unwrap().extend_from_slice(data);
            std::task::Poll::Ready(Ok(data.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn goal_stdio_lifecycle_orders_activation_plan_resume_failure_and_final_answer() {
        use super::super::jsonrpc::{ExitSignalSender, JsonRpcPeer};
        for case in [
            "start",
            "resume",
            "plan",
            "reject",
            "missing",
            "eof",
            "turn-failed",
            "cancel",
        ] {
            let captured = Capture(Default::default());
            let cancel = CancellationToken::new();
            let client = AppServerClient::new(
                LogWriter::new(captured.clone()),
                Some(std::sync::Arc::new(
                    crate::approvals::NoopExecutorApprovalService,
                )),
                false,
                case == "plan",
                if case == "plan" {
                    ExecutionMode::PlanWithGoal
                } else {
                    ExecutionMode::Goal
                },
                None,
                None,
                Default::default(),
                false,
                String::new(),
                cancel.clone(),
            );
            client.register_session("thread-1").await.unwrap();
            let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "executors::codex::client::version_check_tests::goal_stdio_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("EVK_GOAL_FIXTURE", case)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
            let peer = JsonRpcPeer::spawn(
                child.stdin.take().unwrap(),
                child.stdout.take().unwrap(),
                client.clone(),
                ExitSignalSender::new(exit_tx),
                cancel.clone(),
            );
            client.connect(peer.clone());
            let exercise = async {
                peer.send(&json!({"method":"fixture/go"})).await.unwrap();
                if case != "plan" {
                    if case == "cancel" {
                        cancel.cancel();
                    }
                    let result = if case == "resume" || case == "missing" {
                        client.resume_goal("thread-1".into()).await
                    } else {
                        client
                            .start_goal("thread-1".into(), "new objective".into())
                            .await
                    };
                    if ["reject", "missing", "cancel"].contains(&case) {
                        assert!(result.is_err(), "{case}");
                        peer.request_exit(ExecutorExitResult::Failure);
                    } else {
                        assert!(result.is_ok(), "{case}: {result:?}");
                    }
                }
                let exit = exit_rx.await.unwrap();
                assert_eq!(
                    matches!(exit, ExecutorExitResult::Success),
                    ["start", "resume", "plan"].contains(&case),
                    "{case}"
                );
                if ["start", "resume", "plan", "turn-failed"].contains(&case) {
                    assert!(
                        String::from_utf8_lossy(&captured.0.lock().unwrap())
                            .contains("FINAL ANSWER AFTER GOAL COMPLETE"),
                        "{case}: {}",
                        String::from_utf8_lossy(&captured.0.lock().unwrap())
                    );
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), exercise)
                .await
                .unwrap_or_else(|_| panic!("fixture stalled: {case}"));
            cancel.cancel();
            child.kill().await.ok();
            child.wait().await.ok();
        }
    }

    fn goal_client() -> std::sync::Arc<AppServerClient> {
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            false,
            ExecutionMode::Goal,
            None,
            None,
            Default::default(),
            false,
            String::new(),
            CancellationToken::new(),
        );
        *client.thread_id.try_lock().unwrap() = Some("thread-1".into());
        arm_goal(&client);
        client
    }

    fn arm_goal(client: &AppServerClient) {
        let mut state = client.goal_lifecycle.lock().unwrap();
        let id = codex_app_server_protocol::RequestId::Integer(99);
        state.begin(id.clone(), "thread-1".into());
        state.acknowledge(&id, "thread-1", "active").unwrap();
    }

    async fn notify(
        client: &AppServerClient,
        method: &str,
        mut params: serde_json::Value,
    ) -> JsonRpcControlFlow {
        params["threadId"] = json!("thread-1");
        if method == "turn/completed" {
            params["turn"]["items"] = json!([]);
            params["turn"]["error"] = serde_json::Value::Null;
        }
        let raw = json!({"method": method, "params": params}).to_string();
        client
            .process_notification(&raw, serde_json::from_str(&raw).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn goal_completion_drains_tool_and_final_answer_before_exit() {
        let client = goal_client();
        notify(
            &client,
            "thread/goal/updated",
            json!({"goal":{"status":"active"}}),
        )
        .await;
        notify(&client, "turn/started", json!({"turn":{"id":"turn-1"}})).await;
        assert!(matches!(
            notify(
                &client,
                "thread/goal/updated",
                json!({"goal":{"status":"complete"}})
            )
            .await,
            JsonRpcControlFlow::Continue
        ));
        for method in [
            "item/completed",
            "item/agentMessage/delta",
            "item/completed",
        ] {
            assert!(matches!(
                notify(&client, method, json!({})).await,
                JsonRpcControlFlow::Continue
            ));
        }
        assert!(matches!(
            notify(
                &client,
                "turn/completed",
                json!({"turn":{"id":"old-turn","status":"completed"}})
            )
            .await,
            JsonRpcControlFlow::Continue
        ));
        assert!(matches!(
            notify(
                &client,
                "turn/completed",
                json!({"turn":{"id":"turn-1","status":"completed"}})
            )
            .await,
            JsonRpcControlFlow::Exit(ExecutorExitResult::Success)
        ));
    }

    #[tokio::test]
    async fn idle_goal_completion_exits_without_waiting_for_a_nonexistent_turn() {
        let client = goal_client();
        notify(
            &client,
            "thread/goal/updated",
            json!({"goal":{"status":"active"}}),
        )
        .await;
        notify(&client, "turn/started", json!({"turn":{"id":"turn-1"}})).await;
        assert!(matches!(
            notify(
                &client,
                "turn/completed",
                json!({"turn":{"id":"turn-1","status":"completed"}})
            )
            .await,
            JsonRpcControlFlow::Continue
        ));
        assert!(matches!(
            notify(
                &client,
                "thread/goal/updated",
                json!({"goal":{"status":"complete"}})
            )
            .await,
            JsonRpcControlFlow::Exit(ExecutorExitResult::Success)
        ));
    }

    #[tokio::test]
    async fn goal_complete_does_not_hide_turn_failure_or_interruption() {
        for status in ["failed", "interrupted"] {
            let client = goal_client();
            notify(
                &client,
                "thread/goal/updated",
                json!({"goal":{"status":"active"}}),
            )
            .await;
            notify(&client, "turn/started", json!({"turn":{"id":"turn-1"}})).await;
            notify(
                &client,
                "thread/goal/updated",
                json!({"goal":{"status":"complete"}}),
            )
            .await;
            assert!(matches!(
                notify(
                    &client,
                    "turn/completed",
                    json!({"turn":{"id":"turn-1","status":status}})
                )
                .await,
                JsonRpcControlFlow::Exit(ExecutorExitResult::Failure)
            ));
        }
    }

    #[tokio::test]
    async fn clearing_goal_also_drains_an_active_turn_but_exits_when_idle() {
        for active_turn in [false, true] {
            let client = goal_client();
            notify(
                &client,
                "thread/goal/updated",
                json!({"goal":{"status":"active"}}),
            )
            .await;
            if active_turn {
                notify(&client, "turn/started", json!({"turn":{"id":"turn-1"}})).await;
            }
            let result = notify(&client, "thread/goal/cleared", json!({})).await;
            if active_turn {
                assert!(matches!(result, JsonRpcControlFlow::Continue));
                assert!(matches!(
                    notify(
                        &client,
                        "turn/completed",
                        json!({"turn":{"id":"turn-1","status":"completed"}})
                    )
                    .await,
                    JsonRpcControlFlow::Exit(ExecutorExitResult::Success)
                ));
            } else {
                assert!(matches!(
                    result,
                    JsonRpcControlFlow::Exit(ExecutorExitResult::Success)
                ));
            }
        }
    }

    #[test]
    fn resume_goal_preserves_objective_budget_and_accounting() {
        use codex_app_server_protocol::ThreadGoalStatus;
        for status in [
            ThreadGoalStatus::Active,
            ThreadGoalStatus::Paused,
            ThreadGoalStatus::Blocked,
            ThreadGoalStatus::UsageLimited,
            ThreadGoalStatus::BudgetLimited,
        ] {
            let params =
                super::goal_resume_params("existing-thread".into(), Some(&status)).unwrap();
            assert_eq!(params.thread_id, "existing-thread");
            assert!(params.objective.is_none());
            assert!(params.token_budget.is_none());
            assert!(matches!(params.status, Some(ThreadGoalStatus::Active)));
        }
    }

    #[test]
    fn resume_goal_rejects_missing_and_complete_provider_goal() {
        assert!(super::goal_resume_params("thread".into(), None).is_err());
        assert!(
            super::goal_resume_params(
                "thread".into(),
                Some(&codex_app_server_protocol::ThreadGoalStatus::Complete)
            )
            .is_err()
        );
    }

    use codex_protocol::{
        config_types::{CollaborationMode, ModeKind, Settings},
        openai_models::ReasoningEffort as ProtocolReasoningEffort,
    };
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::{
        AppServerClient, ExecutorExitResult, JsonRpcControlFlow, LogWriter, ReasoningEffortUpdate,
        TurnStatus, build_thread_settings_update_params, build_turn_start_params,
        clear_completed_turn, ensure_resumed_thread_id, extract_semver,
        parse_turn_completed_params, reasoning_update_from_thread_settings,
        turn_completion_control_flow,
    };
    use crate::profile::ExecutionMode;

    #[test]
    fn turn_completion_statuses_fail_closed() {
        assert!(matches!(
            turn_completion_control_flow(&TurnStatus::Completed, false),
            JsonRpcControlFlow::Exit(ExecutorExitResult::Success)
        ));
        assert!(matches!(
            turn_completion_control_flow(&TurnStatus::Failed, false),
            JsonRpcControlFlow::Exit(ExecutorExitResult::Failure)
        ));
        assert!(matches!(
            turn_completion_control_flow(&TurnStatus::Interrupted, false),
            JsonRpcControlFlow::Exit(ExecutorExitResult::Failure)
        ));
        assert!(matches!(
            turn_completion_control_flow(&TurnStatus::InProgress, false),
            JsonRpcControlFlow::Exit(ExecutorExitResult::Failure)
        ));
        assert!(matches!(
            turn_completion_control_flow(&TurnStatus::Interrupted, true),
            JsonRpcControlFlow::Continue
        ));
        assert!(matches!(
            turn_completion_control_flow(&TurnStatus::Completed, true),
            JsonRpcControlFlow::Continue
        ));
    }

    #[test]
    fn code_and_plan_ignore_goal_state_replayed_during_thread_resume() {
        for mode in [ExecutionMode::Code, ExecutionMode::Plan] {
            let client = AppServerClient::new(
                LogWriter::new(tokio::io::sink()),
                None,
                false,
                mode == ExecutionMode::Plan,
                mode,
                None,
                None,
                Default::default(),
                false,
                String::new(),
                CancellationToken::new(),
            );

            assert!(client.observe_goal_updated("thread-1", "active").is_none());
            assert!(!client.goal_keeps_run_alive());
            assert!(client.observe_goal_updated("thread-1", "cleared").is_none());
        }
    }

    #[test]
    fn goal_clear_only_finishes_a_goal_that_was_started_by_this_run() {
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            false,
            ExecutionMode::Goal,
            None,
            None,
            Default::default(),
            false,
            String::new(),
            CancellationToken::new(),
        );

        assert!(client.observe_goal_updated("thread-1", "cleared").is_none());
        assert!(client.observe_goal_updated("thread-1", "active").is_none());
        assert!(client.goal_keeps_run_alive());
        arm_goal(&client);
        assert!(client.goal_keeps_run_alive());
        assert!(matches!(
            client.observe_goal_updated("thread-1", "cleared"),
            Some(ExecutorExitResult::Success)
        ));
        assert!(!client.goal_keeps_run_alive());

        assert!(client.observe_goal_updated("thread-1", "active").is_none());
        assert!(matches!(
            client.observe_goal_updated("thread-1", "complete"),
            Some(ExecutorExitResult::Success)
        ));
    }

    #[test]
    fn completed_turn_only_clears_the_matching_active_turn() {
        let mut active = Some("turn-1".to_string());
        clear_completed_turn(&mut active, "turn-2");
        assert_eq!(active.as_deref(), Some("turn-1"));

        clear_completed_turn(&mut active, "turn-1");
        assert_eq!(active, None);
    }

    #[test]
    fn turn_completed_params_must_be_present_and_valid() {
        let params = json!({
            "threadId": "thread-1",
            "turn": {
                "id": "turn-1",
                "items": [],
                "status": "completed",
                "error": null,
                "startedAt": null,
                "completedAt": null,
                "durationMs": null
            }
        });

        let completed = parse_turn_completed_params(Some(&params)).expect("valid completion");
        assert_eq!(completed.turn.status, TurnStatus::Completed);
        assert!(parse_turn_completed_params(None).is_err());

        let malformed = json!({ "turn": { "status": "completed" } });
        assert!(parse_turn_completed_params(Some(&malformed)).is_err());
    }

    #[test]
    fn control_responses_ignore_new_history_variants_but_validate_identity_and_status() {
        let history =
            json!([{"type":"subAgentActivity","kind":"completed"}, {"type":"futureItem"}]);
        let resumed: super::ResumedThread = serde_json::from_value(json!({
            "thread":{"id":"thread-1","turns":[{"items":history.clone()}]}, "model":"model"
        }))
        .unwrap();
        assert_eq!(resumed.thread.id, "thread-1");
        assert!(
            serde_json::from_value::<super::ResumedThread>(json!({"thread":{},"model":"model"}))
                .is_err()
        );
        assert!(
            parse_turn_completed_params(Some(
                &json!({"turn":{"id":"t","status":"completed","items":history}})
            ))
            .is_ok()
        );
        assert!(
            parse_turn_completed_params(Some(&json!({"turn":{"id":"t","status":"futureStatus"}})))
                .is_err()
        );
    }

    #[test]
    fn extracts_version_from_user_agent() {
        assert_eq!(
            extract_semver("codex/0.144.1 (Windows 10; x86_64) vibe-codex-executor"),
            Some("0.144.1".to_string())
        );
        assert_eq!(
            extract_semver("codex/1.2.3-alpha.1"),
            Some("1.2.3".to_string())
        );
        assert_eq!(extract_semver("no version here"), None);
    }

    #[test]
    fn runtime_version_is_observation_only() {
        // Newer and older local CLIs are not rejected here. The app-server
        // handshake and subsequent JSON-RPC calls are the compatibility
        // boundary; this parser is only for diagnostics/audit metadata.
        assert_eq!(extract_semver("codex/0.147.0"), Some("0.147.0".to_string()));
        assert_eq!(extract_semver("codex/0.144.1"), Some("0.144.1".to_string()));
        assert_eq!(extract_semver("unknown"), None);
    }

    #[test]
    fn resumed_thread_must_keep_the_requested_id() {
        assert!(ensure_resumed_thread_id("thread-1", "thread-1").is_ok());

        let error = ensure_resumed_thread_id("thread-1", "thread-2")
            .expect_err("a resume response must not silently become a fork");
        assert!(
            error
                .to_string()
                .contains("requested thread-1, got thread-2")
        );
    }

    // Regression: request_id() must handle every ClientRequest variant the
    // client actually sends, or send_request panics at runtime. model_list()
    // sends ModelList, which was missing from the match.
    #[test]
    fn request_id_handles_model_list() {
        use codex_app_server_protocol::{ClientRequest, ModelListParams, RequestId};

        let req = ClientRequest::ModelList {
            request_id: RequestId::Integer(7),
            params: ModelListParams::default(),
        };
        assert_eq!(super::request_id(&req), RequestId::Integer(7));
    }

    #[test]
    fn request_id_handles_goal_and_settings_requests() {
        use codex_app_server_protocol::{
            ClientRequest, RequestId, ThreadGoalClearParams, ThreadGoalGetParams,
            ThreadGoalSetParams, ThreadSettingsUpdateParams,
        };

        let goal_set = ClientRequest::ThreadGoalSet {
            request_id: RequestId::Integer(8),
            params: ThreadGoalSetParams {
                thread_id: "thread_123".to_string(),
                objective: Some("ship slash commands".to_string()),
                status: None,
                token_budget: None,
            },
        };
        assert_eq!(super::request_id(&goal_set), RequestId::Integer(8));

        let goal_get = ClientRequest::ThreadGoalGet {
            request_id: RequestId::Integer(9),
            params: ThreadGoalGetParams {
                thread_id: "thread_123".to_string(),
            },
        };
        assert_eq!(super::request_id(&goal_get), RequestId::Integer(9));

        let goal_clear = ClientRequest::ThreadGoalClear {
            request_id: RequestId::Integer(10),
            params: ThreadGoalClearParams {
                thread_id: "thread_123".to_string(),
            },
        };
        assert_eq!(super::request_id(&goal_clear), RequestId::Integer(10));

        let settings_update = ClientRequest::ThreadSettingsUpdate {
            request_id: RequestId::Integer(11),
            params: ThreadSettingsUpdateParams {
                thread_id: "thread_123".to_string(),
                ..Default::default()
            },
        };
        assert_eq!(super::request_id(&settings_update), RequestId::Integer(11));
    }

    #[test]
    fn initial_collaboration_mode_includes_configured_reasoning_effort() {
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            false,
            ExecutionMode::Code,
            None,
            Some(ProtocolReasoningEffort::XHigh),
            Default::default(),
            false,
            String::new(),
            CancellationToken::new(),
        );
        client.set_resolved_model("gpt-test".to_string());

        let collaboration_mode = client
            .initial_collaboration_mode()
            .expect("resolved model should build collaboration mode");

        assert_eq!(collaboration_mode.mode, ModeKind::Default);
        assert_eq!(collaboration_mode.settings.model, "gpt-test");
        assert_eq!(
            collaboration_mode.settings.reasoning_effort,
            Some(ProtocolReasoningEffort::XHigh)
        );
    }

    #[test]
    fn turn_start_params_include_explicit_xhigh_effort() {
        let collaboration_mode = CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "gpt-test".to_string(),
                reasoning_effort: Some(ProtocolReasoningEffort::XHigh),
                developer_instructions: None,
            },
        };

        let params = build_turn_start_params(
            "thread_123".to_string(),
            vec![],
            Some(collaboration_mode.clone()),
            Some(ProtocolReasoningEffort::XHigh),
        );

        assert_eq!(params.effort, Some(ProtocolReasoningEffort::XHigh));
        assert_eq!(
            params
                .collaboration_mode
                .as_ref()
                .and_then(|mode| mode.settings.reasoning_effort.clone()),
            Some(ProtocolReasoningEffort::XHigh)
        );
    }

    #[test]
    fn turn_start_params_omit_effort_when_no_override_exists() {
        let params = build_turn_start_params("thread_123".to_string(), vec![], None, None);

        assert_eq!(params.effort, None);
        assert_eq!(params.collaboration_mode, None);
    }

    #[test]
    fn thread_settings_update_params_include_explicit_xhigh_effort() {
        let params = build_thread_settings_update_params(
            "thread_123".to_string(),
            Some(ProtocolReasoningEffort::XHigh),
            None,
        );

        assert_eq!(params.thread_id, "thread_123");
        assert_eq!(params.effort, Some(ProtocolReasoningEffort::XHigh));
        assert_eq!(params.collaboration_mode, None);
    }

    #[test]
    fn live_reasoning_settings_update_sets_effort_in_collaboration_mode() {
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            false,
            ExecutionMode::Code,
            None,
            Some(ProtocolReasoningEffort::High),
            Default::default(),
            false,
            String::new(),
            CancellationToken::new(),
        );
        client.set_resolved_model("gpt-test".to_string());

        let params = client
            .build_reasoning_thread_settings_update_params(
                "thread_123".to_string(),
                Some(ProtocolReasoningEffort::XHigh),
            )
            .expect("resolved model should build settings update");

        assert_eq!(params.effort, Some(ProtocolReasoningEffort::XHigh));
        assert_eq!(
            params.collaboration_mode.as_ref().map(|mode| mode.mode),
            Some(ModeKind::Default)
        );
        assert_eq!(
            params
                .collaboration_mode
                .as_ref()
                .map(|mode| mode.settings.model.as_str()),
            Some("gpt-test")
        );
        assert_eq!(
            params
                .collaboration_mode
                .as_ref()
                .and_then(|mode| mode.settings.reasoning_effort.clone()),
            Some(ProtocolReasoningEffort::XHigh)
        );
    }

    #[test]
    fn live_reasoning_settings_update_can_clear_effort() {
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            false,
            ExecutionMode::Code,
            None,
            Some(ProtocolReasoningEffort::XHigh),
            Default::default(),
            false,
            String::new(),
            CancellationToken::new(),
        );
        client.set_resolved_model("gpt-test".to_string());

        let params = client
            .build_reasoning_thread_settings_update_params("thread_123".to_string(), None)
            .expect("resolved model should build settings update");

        assert_eq!(params.effort, None);
        assert_eq!(
            params.collaboration_mode.as_ref().map(|mode| mode.mode),
            Some(ModeKind::Default)
        );
        assert_eq!(
            params
                .collaboration_mode
                .as_ref()
                .map(|mode| mode.settings.model.as_str()),
            Some("gpt-test")
        );
        assert_eq!(
            params
                .collaboration_mode
                .as_ref()
                .and_then(|mode| mode.settings.reasoning_effort.clone()),
            None
        );
        match reasoning_update_from_thread_settings(&params) {
            ReasoningEffortUpdate::Set(None) => {}
            _ => panic!("clear update should be tracked as an explicit reasoning clear"),
        }
    }

    #[test]
    fn thread_settings_update_params_omit_effort_when_no_override_exists() {
        let params = build_thread_settings_update_params("thread_123".to_string(), None, None);

        assert_eq!(params.effort, None);
        assert_eq!(params.collaboration_mode, None);
    }
}
