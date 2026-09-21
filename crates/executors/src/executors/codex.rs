pub mod client;
pub const GOAL_OBJECTIVE_MAX_CHARS: usize = 4000;

pub fn goal_concurrency_constraint(limit: Option<u16>) -> String {
    match limit {
        Some(0) => {
            "\n\nExecution constraint: do not spawn or delegate to subagents for this goal.".into()
        }
        Some(limit) => format!(
            "\n\nExecution constraint: when work can be divided safely, use at most {limit} concurrent spawned subagents (excluding the primary agent). Avoid concurrent writes to the same files."
        ),
        None => String::new(),
    }
}

pub fn validate_goal_objective(objective: &str) -> Result<(), ExecutorError> {
    let count = objective.chars().count();
    if count > GOAL_OBJECTIVE_MAX_CHARS {
        return Err(ExecutorError::Io(std::io::Error::other(format!(
            "Codex Goal objective is {count} characters; maximum is {GOAL_OBJECTIVE_MAX_CHARS}. Shorten the instructions."
        ))));
    }
    Ok(())
}

fn add_goal_skill_context(params: &mut ThreadStartParams, skills: &[SelectedSkill]) {
    if skills.is_empty() {
        return;
    }
    // Goal activation has no UserInput array. Make the same skill references
    // available before activation, without starting an extra turn or inflating
    // the persistent 4,000-character objective. References are data, not commands.
    let references = serde_json::to_string(skills).expect("skill references serialize");
    let context = format!(
        "Available selected skills (JSON name/path references): {references}\nRead the referenced SKILL.md before using a relevant skill. Choose skills according to the current task; availability does not require invoking a skill."
    );
    params.developer_instructions = Some(match params.developer_instructions.take() {
        Some(existing) => format!("{existing}\n\n{context}"),
        None => context,
    });
}
pub mod agent_scope;
mod goal_lifecycle;

pub mod jsonrpc;
pub mod normalize_logs;
pub mod review;
pub mod slash_commands;
use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};

/// Returns the Codex home directory.
///
/// Checks the `CODEX_HOME` environment variable first, then falls back to `~/.codex`.
/// This allows users to configure a custom location for Codex configuration and state.
pub fn codex_home() -> Option<PathBuf> {
    if let Ok(codex_home) = env::var("CODEX_HOME")
        && !codex_home.trim().is_empty()
    {
        return Some(PathBuf::from(codex_home));
    }
    dirs::home_dir().map(|home| home.join(".codex"))
}

pub(crate) fn resolve_model(model: Option<&str>) -> (Option<&str>, bool) {
    match model.and_then(|m| m.strip_suffix("-fast")) {
        Some(base) => (Some(base), true),
        None => (model, false),
    }
}

pub(crate) fn resume_params_from(
    thread_id: String,
    params: ThreadStartParams,
) -> ThreadResumeParams {
    ThreadResumeParams {
        thread_id,
        model: params.model,
        model_provider: params.model_provider,
        service_tier: params.service_tier,
        cwd: params.cwd,
        runtime_workspace_roots: params.runtime_workspace_roots,
        approval_policy: params.approval_policy,
        approvals_reviewer: params.approvals_reviewer,
        sandbox: params.sandbox,
        permissions: params.permissions,
        config: params.config,
        base_instructions: params.base_instructions,
        developer_instructions: params.developer_instructions,
        personality: params.personality,
        ..Default::default()
    }
}

const SKILLS_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const MODELS_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

use async_trait::async_trait;
use codex_app_server_protocol::{
    AskForApproval as V2AskForApproval, ModelListResponse, ReviewTarget,
    SandboxMode as V2SandboxMode, SkillScope, SkillsListResponse, ThreadResumeParams,
    ThreadStartParams, UserInput,
};
use codex_protocol::{
    config_types::ServiceTier, openai_models::ReasoningEffort as ProtocolReasoningEffort,
};
use derivative::Derivative;
use futures::StreamExt;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum_macros::AsRefStr;
use tokio::process::Command;
use ts_rs::TS;
use workspace_utils::{command_ext::GroupSpawnNoWindowExt, msg_store::MsgStore};

use self::{
    client::{AppServerClient, LogWriter},
    jsonrpc::{ExitSignalSender, JsonRpcPeer},
    normalize_logs::{Error, normalize_logs},
};

pub mod command_adapter;
use crate::{
    actions::SelectedSkill,
    approvals::ExecutorApprovalService,
    command::{CmdOverrides, CommandBuildError, CommandBuilder, CommandParts},
    env::{ExecutionEnv, RepoContext},
    executor_discovery::{CodexSkillDescription, CodexSkillLoadError, ExecutorDiscoveredOptions},
    executors::{
        AppendPrompt, AvailabilityInfo, BaseCodingAgent, ExecutorControl, ExecutorError,
        ExecutorExitResult, SpawnedChild, StandardCodingAgentExecutor,
    },
    logs::utils::patch,
    model_selector::{ModelInfo, ModelSelectorConfig, PermissionPolicy, ReasoningOption},
    profile::{ExecutionMode, ExecutorConfig},
    stdout_dup::create_stdout_pipe_writer,
};

#[derive(Debug)]
struct DeferredCodexControl {
    client: Arc<OnceLock<Arc<AppServerClient>>>,
}

#[async_trait]
impl ExecutorControl for DeferredCodexControl {
    async fn send(
        &self,
        control: crate::executors::provider_adapter::DirectControl,
    ) -> Result<Vec<u8>, ExecutorError> {
        for _ in 0..100 {
            if let Some(client) = self.client.get() {
                return client.send_direct_control(control).await;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err(ExecutorError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Codex app-server control peer was not initialized",
        )))
    }
}

/// Sandbox policy modes for Codex
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema, AsRefStr)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum SandboxMode {
    Auto,
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

/// Determines when the user is consulted to approve Codex actions.
///
/// - `UnlessTrusted`: Read-only commands are auto-approved. Everything else will
///   ask the user to approve.
/// - `OnRequest`: The model decides when to ask the user for approval.
/// - `Never`: Commands never ask for approval. Commands that fail in the
///   restricted sandbox are not retried.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema, AsRefStr)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum AskForApproval {
    UnlessTrusted,
    OnRequest,
    Never,
}

/// Reasoning effort for the underlying model.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
#[ts(type = "string")]
pub struct ReasoningEffort(String);

impl ReasoningEffort {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn to_protocol(&self) -> ProtocolReasoningEffort {
        self.as_str()
            .parse()
            .expect("local Codex reasoning effort is always non-empty")
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ReasoningEffort {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            Err("reasoning effort must not be empty".to_string())
        } else {
            Ok(Self(value.to_string()))
        }
    }
}

impl Serialize for ReasoningEffort {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReasoningEffort {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ReasoningEffort {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ReasoningEffort")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A non-empty reasoning effort value advertised by Codex model discovery or supplied by the user.",
            "minLength": 1,
        })
    }
}

/// Model reasoning summary style
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema, AsRefStr)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ReasoningSummary {
    Auto,
    Concise,
    Detailed,
    None,
}

/// Format for model reasoning summaries
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS, JsonSchema, AsRefStr)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ReasoningSummaryFormat {
    None,
    Experimental,
}

enum CodexSessionAction {
    Chat {
        prompt: String,
        selected_skills: Vec<SelectedSkill>,
    },
    Review {
        target: ReviewTarget,
    },
}

#[derive(Derivative, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[derivative(Debug, PartialEq)]
pub struct Codex {
    #[serde(default)]
    pub append_prompt: AppendPrompt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_for_approval: Option<AskForApproval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oss: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_reasoning_summary: Option<ReasoningSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_reasoning_summary_format: Option<ReasoningSummaryFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_apply_patch_tool: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_instructions: Option<String>,
    #[serde(default)]
    pub plan: bool,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_token_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_max_concurrent_agents: Option<u16>,
    #[serde(flatten)]
    pub cmd: CmdOverrides,

    #[serde(skip)]
    #[ts(skip)]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    approvals: Option<Arc<dyn ExecutorApprovalService>>,
}

#[async_trait]
impl StandardCodingAgentExecutor for Codex {
    fn apply_overrides(&mut self, executor_config: &ExecutorConfig) {
        self.apply_direct_overrides(executor_config);
    }

    fn use_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.use_direct_approvals(approvals);
    }

    async fn spawn(
        &self,
        current_dir: &Path,
        prompt: &str,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        self.spawn_slash_command(current_dir, prompt, None, vec![], env)
            .await
    }

    async fn spawn_with_selected_skills(
        &self,
        current_dir: &Path,
        prompt: &str,
        selected_skills: &[SelectedSkill],
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        self.spawn_slash_command(current_dir, prompt, None, selected_skills.to_vec(), env)
            .await
    }

    async fn spawn_follow_up(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        _reset_to_message_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        self.spawn_slash_command(current_dir, prompt, Some(session_id), vec![], env)
            .await
    }

    async fn spawn_follow_up_with_selected_skills(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: &str,
        _reset_to_message_id: Option<&str>,
        selected_skills: &[SelectedSkill],
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        self.spawn_slash_command(
            current_dir,
            prompt,
            Some(session_id),
            selected_skills.to_vec(),
            env,
        )
        .await
    }

    fn normalize_logs(
        &self,
        msg_store: Arc<MsgStore>,
        worktree_path: &Path,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        normalize_logs(msg_store, worktree_path)
    }

    fn default_mcp_config_path(&self) -> Option<PathBuf> {
        codex_home().map(|home| home.join("config.toml"))
    }

    fn get_availability_info(&self) -> AvailabilityInfo {
        if !crate::command::is_command_installed(
            crate::command::CODEX_DEFAULT_BASE_COMMAND,
            &self.cmd,
        ) {
            return AvailabilityInfo::NotFound;
        }
        if let Some(timestamp) = codex_home()
            .and_then(|home| std::fs::metadata(home.join("auth.json")).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
        {
            return AvailabilityInfo::LoginDetected {
                last_auth_timestamp: timestamp,
            };
        }

        AvailabilityInfo::InstallationFound
    }

    fn get_preset_options(&self) -> ExecutorConfig {
        use crate::model_selector::*;
        let permission_policy = if self.plan {
            PermissionPolicy::Plan
        } else if matches!(self.ask_for_approval, None | Some(AskForApproval::Never)) {
            PermissionPolicy::Auto
        } else {
            PermissionPolicy::Supervised
        };

        ExecutorConfig {
            executor: BaseCodingAgent::Codex,
            variant: None,
            model_id: self.model.clone(),
            agent_id: None,
            reasoning_id: self
                .model_reasoning_effort
                .as_ref()
                .map(|e| e.as_str().to_string()),
            permission_policy: Some(permission_policy),
            execution_mode: Some(if self.plan {
                ExecutionMode::Plan
            } else {
                self.execution_mode
            }),
            goal_token_budget: self.goal_token_budget,
            goal_max_concurrent_agents: self.goal_max_concurrent_agents,
        }
    }

    async fn discover_options(
        &self,
        workdir: Option<&std::path::Path>,
        repo_path: Option<&std::path::Path>,
    ) -> Result<futures::stream::BoxStream<'static, json_patch::Patch>, ExecutorError> {
        let skills_cwd = workdir.or(repo_path).map(Path::to_path_buf);
        let mut options = ExecutorDiscoveredOptions {
            model_selector: ModelSelectorConfig {
                models: fallback_models(),
                permissions: vec![
                    PermissionPolicy::Auto,
                    PermissionPolicy::Supervised,
                    PermissionPolicy::Plan,
                ],
                ..Default::default()
            },
            slash_commands: slash_commands::supported_slash_commands(),
            ..Default::default()
        };
        options.loading_models = true;
        options.loading_slash_commands = true;
        options.loading_skills = skills_cwd.is_some();
        let initial_patch = patch::executor_discovered_options(options);

        let this = self.clone();
        let models_cwd = skills_cwd.clone();
        let discovery_stream = async_stream::stream! {
            let slash_commands = slash_commands::supported_slash_commands();
            yield patch::update_slash_commands(slash_commands);
            yield patch::slash_commands_loaded();

            // Models: prefer the live `model/list` over the hardcoded fallback so
            // new Codex models appear automatically. Run against the resolved cwd
            // (config like model providers can be workspace-scoped).
            let model_dir = models_cwd
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let models_result = tokio::time::timeout(
                MODELS_DISCOVERY_TIMEOUT,
                this.discover_models(model_dir),
            )
            .await;
            match models_result {
                Ok(Ok(models)) if !models.is_empty() => {
                    yield patch::update_models(models);
                }
                Ok(Ok(_)) => {
                    tracing::warn!("Codex model/list returned no models; keeping fallback list");
                }
                Ok(Err(err)) => {
                    tracing::warn!("Failed to discover Codex models: {err}; keeping fallback list");
                }
                Err(_) => {
                    tracing::warn!("Timed out discovering Codex models; keeping fallback list");
                }
            }
            yield patch::models_loaded();

            let Some(skills_cwd) = skills_cwd else {
                return;
            };
            let result = tokio::time::timeout(
                SKILLS_DISCOVERY_TIMEOUT,
                this.discover_skills(skills_cwd),
            )
            .await;

            match result {
                Ok(Ok((skills, errors))) => {
                    let mut slash_commands = slash_commands::supported_slash_commands();
                    slash_commands.extend(slash_commands::skill_slash_commands(&skills));
                    yield patch::update_slash_commands(slash_commands);
                    yield patch::update_skills(skills);
                    yield patch::update_skill_errors(errors);
                }
                Ok(Err(err)) => {
                    tracing::warn!("Failed to discover Codex skills: {err}");
                    yield patch::update_skill_errors(vec![CodexSkillLoadError {
                        path: PathBuf::new(),
                        message: format!("Failed to discover Codex skills: {err}"),
                    }]);
                }
                Err(_) => {
                    yield patch::update_skill_errors(vec![CodexSkillLoadError {
                        path: PathBuf::new(),
                        message: "Timed out discovering Codex skills".to_string(),
                    }]);
                }
            }
            yield patch::skills_loaded();
        };

        Ok(Box::pin(
            futures::stream::once(async move { initial_patch }).chain(discovery_stream),
        ))
    }

    async fn spawn_review(
        &self,
        current_dir: &Path,
        prompt: &str,
        session_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let review_target = ReviewTarget::Custom {
            instructions: prompt.to_string(),
        };
        self.spawn_review_target(current_dir, review_target, session_id, env)
            .await
    }
}

impl Codex {
    pub(crate) fn effective_execution_mode(&self) -> ExecutionMode {
        if self.plan && self.execution_mode == ExecutionMode::Code {
            ExecutionMode::Plan
        } else {
            self.execution_mode
        }
    }

    pub(crate) fn prepare_execution_prompt(&self, prompt: String) -> String {
        let prompt = format!(
            "{prompt}{}",
            goal_concurrency_constraint(self.goal_max_concurrent_agents)
        );
        if self.effective_execution_mode() == ExecutionMode::PlanWithGoal {
            format!(
                "{prompt}\n\nProduce an approval-ready goal contract as the final plan, with sections named Outcome, Constraints, Implementation Plan, and Verification and Completion Criteria. The approved text will become the persistent Goal objective, so make it self-contained and include measurable completion conditions."
            )
        } else {
            prompt
        }
    }

    /// V1 direct-runtime metadata and decoder entry point.
    pub const DIRECT_PROVIDER: super::provider_adapter::DirectProvider =
        super::provider_adapter::DirectProvider::Codex;

    pub fn direct_versions() -> super::provider_adapter::DirectAdapterVersions {
        Self::DIRECT_PROVIDER.versions()
    }

    pub fn direct_capabilities(
        runtime_profile_id: impl Into<String>,
    ) -> crate::runtime::CapabilitySnapshot {
        Self::DIRECT_PROVIDER.capabilities(runtime_profile_id)
    }

    pub fn decode_native_frame(
        frame: &crate::runtime::NativeAuditFrame,
    ) -> Result<super::provider_adapter::DecodedProviderEvent, crate::runtime::NativeAuditError>
    {
        Self::DIRECT_PROVIDER.decode_native_frame(frame)
    }

    pub fn direct_mapper() -> super::provider_adapter::DirectProviderMapper {
        Self::DIRECT_PROVIDER.mapper()
    }

    pub(crate) fn apply_direct_overrides(&mut self, executor_config: &ExecutorConfig) {
        if let Some(model_id) = &executor_config.model_id {
            self.model = Some(model_id.clone());
        }
        if let Some(reasoning_id) = &executor_config.reasoning_id
            && let Ok(reasoning_effort) = reasoning_id.parse()
        {
            self.model_reasoning_effort = Some(reasoning_effort)
        }
        if let Some(permission_policy) = &executor_config.permission_policy {
            match permission_policy {
                crate::model_selector::PermissionPolicy::Auto => {
                    self.ask_for_approval = Some(AskForApproval::Never);
                    self.plan = false;
                }
                crate::model_selector::PermissionPolicy::Supervised => {
                    if matches!(self.ask_for_approval, None | Some(AskForApproval::Never)) {
                        self.ask_for_approval = Some(AskForApproval::UnlessTrusted);
                    }
                    self.plan = false;
                }
                crate::model_selector::PermissionPolicy::Plan => {
                    self.plan = true;
                    if executor_config.execution_mode.is_none() {
                        self.execution_mode = ExecutionMode::Plan;
                    }
                }
            }
        }
        if let Some(execution_mode) = executor_config.execution_mode {
            self.execution_mode = execution_mode;
            self.plan = matches!(
                execution_mode,
                ExecutionMode::Plan | ExecutionMode::PlanWithGoal
            );
        }
        self.goal_token_budget = executor_config.goal_token_budget;
        self.goal_max_concurrent_agents = executor_config.goal_max_concurrent_agents;
    }

    pub(crate) fn use_direct_approvals(&mut self, approvals: Arc<dyn ExecutorApprovalService>) {
        self.approvals = Some(approvals);
    }

    pub(crate) async fn launch_direct(
        &self,
        intent: super::provider_adapter::DirectIntent,
        current_dir: &Path,
        prompt: &str,
        session_id: Option<&str>,
        selected_skills: &[SelectedSkill],
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        match intent {
            super::provider_adapter::DirectIntent::Initial => {
                self.spawn_slash_command(current_dir, prompt, None, selected_skills.to_vec(), env)
                    .await
            }
            super::provider_adapter::DirectIntent::FollowUp
            | super::provider_adapter::DirectIntent::Resume => {
                self.spawn_slash_command(
                    current_dir,
                    prompt,
                    session_id,
                    selected_skills.to_vec(),
                    env,
                )
                .await
            }
            super::provider_adapter::DirectIntent::Review => {
                self.spawn_review_target(
                    current_dir,
                    ReviewTarget::Custom {
                        instructions: prompt.to_string(),
                    },
                    session_id,
                    env,
                )
                .await
            }
        }
    }

    async fn spawn_review_target(
        &self,
        current_dir: &Path,
        review_target: ReviewTarget,
        session_id: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let command_parts = self.build_command_builder()?.build_initial()?;
        let action = CodexSessionAction::Review {
            target: review_target,
        };
        self.spawn_inner(current_dir, command_parts, action, session_id, env)
            .await
    }
}

impl Codex {
    pub const DEFAULT_BASE_COMMAND: &'static str = "codex";
    const DISABLE_NATIVE_MEMORY_ARGS: [&'static str; 6] = [
        "-c",
        "features.memories=false",
        "-c",
        "memories.generate_memories=false",
        "-c",
        "memories.use_memories=false",
    ];

    /// Resolve the user's locally installed Codex executable from PATH.
    ///
    /// The app-server protocol crate is a compile-time type dependency; it
    /// does not authorize replacing the user's runtime with a pinned npm
    /// binary. Explicit command overrides remain supported by `CmdOverrides`.
    pub fn base_command() -> String {
        Self::DEFAULT_BASE_COMMAND.to_string()
    }

    fn launch_context(&self, program_path: &Path, args: &[String], current_dir: &Path) -> String {
        let codex_config = codex_home()
            .map(|home| home.join("config.toml").display().to_string())
            .unwrap_or_else(|| "<unresolved>".to_string());

        let mut lines = vec![
            format!(
                "base command: {}",
                self.cmd
                    .base_command_override
                    .clone()
                    .unwrap_or_else(Self::base_command)
            ),
            format!(
                "base_command_override: {}",
                if self.cmd.base_command_override.is_some() {
                    "set"
                } else {
                    "not set"
                }
            ),
            format!("resolved executable: {}", program_path.display()),
            format!("args: {args:?}"),
            format!("cwd: {}", current_dir.display()),
            format!("codex config: {codex_config}"),
        ];

        if let Some(model) = &self.model {
            lines.push(format!("model: {model}"));
        }
        if let Some(model_provider) = &self.model_provider {
            lines.push(format!("model_provider: {model_provider}"));
        }
        if let Some(profile) = &self.profile {
            lines.push(format!("profile: {profile}"));
        }
        if let Some(additional_params) = &self.cmd.additional_params {
            lines.push(format!("additional_params: {additional_params:?}"));
        }

        lines.join("\n")
    }

    fn build_command_builder(&self) -> Result<CommandBuilder, CommandBuildError> {
        command_adapter::CodexCommandAdapter::new(self).build()
    }

    async fn discover_skills(
        &self,
        cwd: PathBuf,
    ) -> Result<(Vec<CodexSkillDescription>, Vec<CodexSkillLoadError>), ExecutorError> {
        let request_cwd = cwd.clone();
        let response = self
            .with_discovery_app_server(&cwd, move |client| async move {
                client.skills_list(request_cwd).await
            })
            .await?;
        Ok(skills_list_response_to_discovery(response))
    }

    async fn discover_models(&self, cwd: PathBuf) -> Result<Vec<ModelInfo>, ExecutorError> {
        let response = self
            .with_discovery_app_server(&cwd, move |client| async move { client.model_list().await })
            .await?;
        Ok(model_list_response_to_model_infos(response))
    }

    async fn with_discovery_app_server<T, F, Fut>(
        &self,
        current_dir: &Path,
        task: F,
    ) -> Result<T, ExecutorError>
    where
        F: FnOnce(Arc<AppServerClient>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ExecutorError>>,
    {
        let command_parts = self.build_command_builder()?.build_initial()?;
        let (program_path, args) = command_parts.into_resolved().await?;
        let launch_context = self.launch_context(&program_path, &args, current_dir);

        let mut process = Command::new(&program_path);
        process
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(current_dir)
            .env("NPM_CONFIG_LOGLEVEL", "error")
            .env("NODE_NO_WARNINGS", "1")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "error")
            .args(&args);

        ExecutionEnv::new(RepoContext::default(), false, String::new())
            .with_profile(&self.cmd)
            .apply_to_command(&mut process);

        let mut child = process.group_spawn_no_window().map_err(|err| {
            ExecutorError::Io(std::io::Error::other(format!(
                "failed to spawn Codex app-server for discovery: {err}\n\nCodex launch context:\n{}",
                launch_context
            )))
        })?;

        let child_stdout = child.inner().stdout.take().ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other("Codex app server missing stdout"))
        })?;
        let child_stdin = child.inner().stdin.take().ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other("Codex app server missing stdin"))
        })?;

        let cancel = tokio_util::sync::CancellationToken::new();
        let (exit_signal_tx, _exit_signal_rx) = tokio::sync::oneshot::channel();
        let exit_signal_tx = ExitSignalSender::new(exit_signal_tx);
        let client = AppServerClient::new(
            LogWriter::new(tokio::io::sink()),
            None,
            false,
            self.plan,
            ExecutionMode::Code,
            None,
            None,
            RepoContext::default(),
            false,
            String::new(),
            cancel.clone(),
        );
        let rpc_peer = JsonRpcPeer::spawn(
            child_stdin,
            child_stdout,
            client.clone(),
            exit_signal_tx,
            cancel.clone(),
        );
        client.connect(rpc_peer);

        let result = async {
            client.initialize().await?;
            task(client).await
        }
        .await;

        cancel.cancel();
        let _ = child.kill().await;

        result
    }

    fn build_thread_start_params_with_resources(
        &self,
        cwd: &Path,
        env: &ExecutionEnv,
    ) -> ThreadStartParams {
        let mut params = self.build_thread_start_params(cwd);
        let reviewer = env
            .get("EVK_OPENWIKI_REVIEWER")
            .is_some_and(|value| value == "1");
        if reviewer {
            // A trusted server role, not a prompt or user profile preference.
            // Overrides apply to this fresh thread only; inherited user/project
            // MCP and Skill installation remains untouched for other workspaces.
            params.sandbox = Some(codex_app_server_protocol::SandboxMode::ReadOnly);
            params.approval_policy = Some(V2AskForApproval::Never);
            params.permissions = None;
            params.runtime_workspace_roots = None;
            let config = params.config.get_or_insert_with(HashMap::new);
            config.insert("mcp_servers.openwiki.enabled".into(), Value::Bool(false));
            config.insert(
                "skills.config".into(),
                serde_json::json!([{"name":"openwiki", "enabled":false}]),
            );
        } else if env
            .get("EVK_OPENWIKI_MAINTENANCE")
            .is_some_and(|value| value == "1")
        {
            // Public Codex MCP configuration, scoped to this maintenance
            // thread. This also works with custom CODEX_HOME and a parent
            // workspace cwd, where project-level discovery alone is insufficient.
            let config = params.config.get_or_insert_with(HashMap::new);
            // Writer tools are a prerequisite, not an optional integration.
            // Codex may omit slow optional servers from its initial tool catalog.
            // Keep this thread-local; never enable writer tools for the reviewer.
            config.insert("mcp_servers.openwiki.enabled".into(), Value::Bool(true));
            config.insert("mcp_servers.openwiki.required".into(), Value::Bool(true));
            config.insert(
                "mcp_servers.openwiki.startup_timeout_sec".into(),
                Value::from(10),
            );
            config.insert(
                "mcp_servers.openwiki.command".into(),
                Value::String("openwiki".into()),
            );
            config.insert(
                "mcp_servers.openwiki.args".into(),
                serde_json::json!(["mcp", "--host", "codex"]),
            );
            config.insert(
                "mcp_servers.openwiki.env".into(),
                serde_json::json!({"OPENWIKI_TELEMETRY_DISABLED":"1"}),
            );
        }
        if !reviewer
            && env
                .get("EVK_OPENWIKI_MAINTENANCE")
                .is_none_or(|value| value != "1")
        {
            params
                .config
                .get_or_insert_with(HashMap::new)
                .insert("mcp_servers.openwiki.enabled".into(), Value::Bool(false));
            params.developer_instructions = Some(match params.developer_instructions.take() {
                Some(existing) => {
                    format!("{existing}\n\n{}", crate::legacy_wiki::RETIREMENT_NOTICE)
                }
                None => crate::legacy_wiki::RETIREMENT_NOTICE.into(),
            });
        }
        if let Some(memory) = env
            .get("EVK_REPOSITORY_MEMORY_INSTRUCTIONS")
            .filter(|_| !reviewer)
        {
            params.developer_instructions = Some(match params.developer_instructions.take() {
                Some(existing) => format!("{existing}\n\n{memory}"),
                None => memory.clone(),
            });
        }
        let roots = env.shared_resource_roots();
        if !reviewer && !roots.is_empty() {
            params.runtime_workspace_roots = Some(
                std::iter::once(cwd.to_path_buf())
                    .chain(roots)
                    .filter_map(|path| path.try_into().ok())
                    .collect(),
            );
        }
        params
    }

    fn build_thread_start_params(&self, cwd: &Path) -> ThreadStartParams {
        let sandbox = match self.sandbox.as_ref() {
            None | Some(SandboxMode::Auto) => Some(V2SandboxMode::WorkspaceWrite), // match the Auto preset in codex
            Some(SandboxMode::ReadOnly) => Some(V2SandboxMode::ReadOnly),
            Some(SandboxMode::WorkspaceWrite) => Some(V2SandboxMode::WorkspaceWrite),
            Some(SandboxMode::DangerFullAccess) => Some(V2SandboxMode::DangerFullAccess),
        };

        let approval_policy = match self.ask_for_approval.as_ref() {
            None if matches!(self.sandbox.as_ref(), None | Some(SandboxMode::Auto)) => {
                // match the Auto preset in codex
                Some(V2AskForApproval::OnRequest)
            }
            None => None,
            Some(AskForApproval::UnlessTrusted) => Some(V2AskForApproval::UnlessTrusted),
            Some(AskForApproval::OnRequest) => Some(V2AskForApproval::OnRequest),
            Some(AskForApproval::Never) => Some(V2AskForApproval::Never),
        };

        let mut config = self.build_config_overrides();
        // V1 top-level params that moved into config overrides in v2
        if let Some(profile) = &self.profile {
            config
                .get_or_insert_with(HashMap::new)
                .insert("profile".to_string(), Value::String(profile.clone()));
        }
        if let Some(include) = self.include_apply_patch_tool {
            config
                .get_or_insert_with(HashMap::new)
                .insert("include_apply_patch_tool".to_string(), Value::Bool(include));
        }
        if let Some(compact) = &self.compact_prompt {
            config
                .get_or_insert_with(HashMap::new)
                .insert("compact_prompt".to_string(), Value::String(compact.clone()));
        }
        let (model, is_fast) = resolve_model(self.model.as_deref());
        let service_tier = if is_fast {
            Some(Some(ServiceTier::Fast.request_value().to_string()))
        } else {
            None
        };

        ThreadStartParams {
            model: model.map(|m| m.to_string()),
            cwd: Some(cwd.to_string_lossy().to_string()),
            approval_policy,
            sandbox,
            config,
            base_instructions: self.base_instructions.clone(),
            model_provider: self.model_provider.clone(),
            developer_instructions: self.developer_instructions.clone(),
            service_tier,
            ..Default::default()
        }
    }

    fn build_config_overrides(&self) -> Option<HashMap<String, Value>> {
        let mut overrides = HashMap::new();

        match self.goal_max_concurrent_agents {
            Some(0) => {
                overrides.insert("agents.enabled".to_string(), Value::Bool(false));
            }
            Some(limit) => {
                overrides.insert("agents.enabled".to_string(), Value::Bool(true));
                overrides.insert(
                    "agents.max_concurrent_threads_per_session".to_string(),
                    Value::from(limit),
                );
            }
            None => {}
        }

        if let Some(effort) = &self.model_reasoning_effort {
            overrides.insert(
                "model_reasoning_effort".to_string(),
                Value::String(effort.as_str().to_string()),
            );
        }

        // The canonical conversation can only display reasoning that Codex
        // deliberately publishes as a summary. Ask for the provider's safe
        // automatic summary unless the profile explicitly selects another
        // mode (including `none`).
        let summary = self
            .model_reasoning_summary
            .as_ref()
            .unwrap_or(&ReasoningSummary::Auto);
        overrides.insert(
            "model_reasoning_summary".to_string(),
            Value::String(summary.as_ref().to_string()),
        );

        if let Some(format) = &self.model_reasoning_summary_format
            && format != &ReasoningSummaryFormat::None
        {
            overrides.insert(
                "model_reasoning_summary_format".to_string(),
                Value::String(format.as_ref().to_string()),
            );
        }

        if overrides.is_empty() {
            None
        } else {
            Some(overrides)
        }
    }

    async fn spawn_inner(
        &self,
        current_dir: &Path,
        command_parts: CommandParts,
        action: CodexSessionAction,
        resume_session: Option<&str>,
        env: &ExecutionEnv,
    ) -> Result<SpawnedChild, ExecutorError> {
        let params = self.build_thread_start_params_with_resources(current_dir, env);
        let resume_session = resume_session.map(|s| s.to_string());

        self.spawn_app_server(
            current_dir,
            command_parts,
            env,
            move |client, _| async move {
                match action {
                    CodexSessionAction::Chat {
                        prompt,
                        selected_skills,
                    } => {
                        Self::launch_codex_agent(
                            params,
                            resume_session,
                            prompt,
                            selected_skills,
                            client,
                        )
                        .await
                    }
                    CodexSessionAction::Review { target } => {
                        review::launch_codex_review(params, resume_session, target, client).await
                    }
                }
            },
        )
        .await
    }

    async fn launch_codex_agent(
        mut thread_start_params: ThreadStartParams,
        resume_session: Option<String>,
        combined_prompt: String,
        selected_skills: Vec<SelectedSkill>,
        client: Arc<AppServerClient>,
    ) -> Result<(), ExecutorError> {
        let requires_openwiki = thread_start_params.config.as_ref().is_some_and(|config| {
            config.get("mcp_servers.openwiki.required") == Some(&Value::Bool(true))
                && config.get("mcp_servers.openwiki.enabled") != Some(&Value::Bool(false))
        });
        if client.execution_mode() == ExecutionMode::Goal {
            let skills = crate::legacy_wiki::filter_skills(selected_skills.clone());
            add_goal_skill_context(&mut thread_start_params, &skills);
        }
        let account = client.get_account().await?;
        if account.requires_openai_auth && account.account.is_none() {
            return Err(ExecutorError::AuthRequired(
                "Codex authentication required".to_string(),
            ));
        }

        let thread_id = match resume_session {
            None => {
                let response = client.thread_start(thread_start_params).await?;
                response.thread.id
            }
            Some(session_id) => {
                let response = client
                    .thread_resume(resume_params_from(session_id, thread_start_params))
                    .await?;
                tracing::debug!("resumed thread, thread_id={}", response.thread.id);
                response.thread.id
            }
        };

        client.register_session(&thread_id).await?;
        if requires_openwiki {
            // A configured/ready server is not proof that the required tools
            // are exposed. Check before either a normal turn or Goal activation.
            client.ensure_openwiki_tools().await?;
        }
        if client.execution_mode() == ExecutionMode::Goal {
            client.start_goal(thread_id, combined_prompt).await?;
            return Ok(());
        }
        let collaboration_mode = client.initial_collaboration_mode()?;
        let input = build_chat_input(combined_prompt, selected_skills);
        client
            .turn_start_with_mode(thread_id, input, Some(collaboration_mode))
            .await?;

        Ok(())
    }

    /// Common boilerplate for spawning a Codex app server process
    /// Handles process spawning, stdout/stderr piping, exit signal handling, client initialization, and error logging.
    /// Delegates the actual Codex session logic to the provided `task` closure.
    async fn spawn_app_server<F, Fut>(
        &self,
        current_dir: &Path,
        command_parts: CommandParts,
        env: &ExecutionEnv,
        task: F,
    ) -> Result<SpawnedChild, ExecutorError>
    where
        F: FnOnce(Arc<AppServerClient>, ExitSignalSender) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), ExecutorError>> + Send + 'static,
    {
        let (program_path, args) = command_parts.into_resolved().await?;
        let launch_context = self.launch_context(&program_path, &args, current_dir);
        tracing::debug!("Launching Codex app-server:\n{}", launch_context);

        let mut process = Command::new(&program_path);
        process
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .current_dir(current_dir)
            .env("NPM_CONFIG_LOGLEVEL", "error")
            .env("NODE_NO_WARNINGS", "1")
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "error")
            .args(&args);

        env.clone()
            .with_profile(&self.cmd)
            .apply_to_command(&mut process);

        let spawn_error_context = launch_context.clone();
        let mut child = process.group_spawn_no_window().map_err(|err| {
            ExecutorError::Io(std::io::Error::other(format!(
                "failed to spawn Codex app-server: {err}\n\nCodex launch context:\n{}",
                spawn_error_context
            )))
        })?;

        let child_stdout = child.inner().stdout.take().ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other("Codex app server missing stdout"))
        })?;
        let child_stdin = child.inner().stdin.take().ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other("Codex app server missing stdin"))
        })?;

        let new_stdout = create_stdout_pipe_writer(&mut child)?;
        let (exit_signal_tx, exit_signal_rx) = tokio::sync::oneshot::channel();
        let cancel = tokio_util::sync::CancellationToken::new();

        let auto_approve = matches!(
            (&self.sandbox, &self.ask_for_approval),
            (Some(SandboxMode::DangerFullAccess), None)
        );
        let execution_mode = self.effective_execution_mode();
        let plan_mode = matches!(
            execution_mode,
            ExecutionMode::Plan | ExecutionMode::PlanWithGoal
        );
        let goal_token_budget = self.goal_token_budget.map(i64::from);
        let reasoning_effort = self
            .model_reasoning_effort
            .as_ref()
            .map(ReasoningEffort::to_protocol);
        let approvals = self.approvals.clone();
        let repo_context = env.repo_context.clone();
        let commit_reminder = env.commit_reminder;
        let commit_reminder_prompt = env.commit_reminder_prompt.clone();
        let cancel_for_task = cancel.clone();
        let client_slot = Arc::new(OnceLock::new());
        let control = Arc::new(DeferredCodexControl {
            client: client_slot.clone(),
        });

        tokio::spawn(async move {
            let exit_signal_tx = ExitSignalSender::new(exit_signal_tx);
            let log_writer = LogWriter::new(new_stdout);

            // Initialize the AppServerClient
            let client = AppServerClient::new(
                log_writer.clone(),
                approvals,
                auto_approve,
                plan_mode,
                execution_mode,
                goal_token_budget,
                reasoning_effort,
                repo_context,
                commit_reminder,
                commit_reminder_prompt,
                cancel_for_task.clone(),
            );
            let rpc_peer = JsonRpcPeer::spawn(
                child_stdin,
                child_stdout,
                client.clone(),
                exit_signal_tx.clone(),
                cancel_for_task,
            );
            client.connect(rpc_peer.clone());
            // Do not expose the control peer until it owns a connected RPC
            // transport.  The process host can send cancellation immediately
            // after launch, before the initialization request has completed.
            let _ = client_slot.set(client.clone());

            let result = async {
                client.initialize().await?;
                task(client, exit_signal_tx.clone()).await
            }
            .await;

            if let Err(err) = result {
                match &err {
                    ExecutorError::Io(io_err)
                        if io_err.kind() == std::io::ErrorKind::BrokenPipe =>
                    {
                        // Transport failure cannot be treated as successful startup.
                        rpc_peer.request_exit(ExecutorExitResult::Failure);
                        return;
                    }
                    ExecutorError::AuthRequired(message) => {
                        log_writer
                            .log_raw(&Error::auth_required(message.clone()).raw())
                            .await
                            .ok();
                        rpc_peer.request_exit(ExecutorExitResult::Failure);
                        return;
                    }
                    _ => {
                        tracing::error!("Codex spawn error: {}", err);
                        let error = format!("{}\n\nCodex launch context:\n{}", err, launch_context);
                        log_writer
                            .log_raw(&Error::launch_error(error).raw())
                            .await
                            .ok();
                    }
                }
                rpc_peer.request_exit(ExecutorExitResult::Failure);
            }
        });

        Ok(SpawnedChild {
            child,
            exit_signal: Some(exit_signal_rx),
            cancel: Some(cancel),
            control: Some(control),
        })
    }
}

fn build_chat_input(
    combined_prompt: String,
    selected_skills: Vec<SelectedSkill>,
) -> Vec<UserInput> {
    let selected_skills = crate::legacy_wiki::filter_skills(selected_skills);
    let mut input = selected_skills
        .into_iter()
        .map(|skill| UserInput::Skill {
            name: skill.name,
            path: skill.path,
        })
        .collect::<Vec<_>>();
    input.push(UserInput::Text {
        text: combined_prompt,
        text_elements: vec![],
    });
    input
}

fn skills_list_response_to_discovery(
    response: SkillsListResponse,
) -> (Vec<CodexSkillDescription>, Vec<CodexSkillLoadError>) {
    let mut skills = Vec::new();
    let mut errors = Vec::new();

    for entry in response.data {
        for skill in entry.skills {
            skills.push(CodexSkillDescription {
                name: skill.name,
                description: skill.description,
                short_description: skill
                    .interface
                    .and_then(|interface| interface.short_description)
                    .or(skill.short_description),
                path: skill.path.to_path_buf(),
                scope: skill_scope_to_string(skill.scope).to_string(),
                enabled: skill.enabled,
            });
        }

        for error in entry.errors {
            errors.push(CodexSkillLoadError {
                path: error.path,
                message: error.message,
            });
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    errors.sort_by(|a, b| a.path.cmp(&b.path));
    (skills, errors)
}

fn skill_scope_to_string(scope: SkillScope) -> &'static str {
    match scope {
        SkillScope::User => "user",
        SkillScope::Repo => "repo",
        SkillScope::System => "system",
        SkillScope::Admin => "admin",
    }
}

/// Convert a live `model/list` response into the UI model-selector shape,
/// dropping models hidden from the default picker.
fn model_list_response_to_model_infos(response: ModelListResponse) -> Vec<ModelInfo> {
    response
        .data
        .into_iter()
        .filter(|model| !model.hidden)
        .map(|model| {
            let default_reasoning_effort = model.default_reasoning_effort.to_string();
            let reasoning_options = ReasoningOption::from_names_with_default(
                model
                    .supported_reasoning_efforts
                    .iter()
                    .map(|option| option.reasoning_effort.to_string()),
                Some(&default_reasoning_effort),
            );
            ModelInfo {
                id: model.id,
                name: model.display_name,
                provider_id: None,
                reasoning_options,
            }
        })
        .collect()
}

/// Static model list used before/instead of a live `model/list` response
/// (discovery failure, timeout, or empty result).
fn fallback_models() -> Vec<ModelInfo> {
    let xhigh_reasoning_options =
        ReasoningOption::from_names(["none", "minimal", "low", "medium", "high", "xhigh"]);
    let max_reasoning_options =
        ReasoningOption::from_names(["low", "medium", "high", "xhigh", "max"]);
    let ultra_reasoning_options =
        ReasoningOption::from_names(["low", "medium", "high", "xhigh", "max", "ultra"]);

    let mut models = [
        ("gpt-5.5", "GPT-5.5"),
        ("gpt-5.5-fast", "GPT-5.5 Fast"),
        ("gpt-5.4", "GPT-5.4"),
        ("gpt-5.4-fast", "GPT-5.4 Fast"),
        ("gpt-5.4-mini", "GPT-5.4 Mini"),
        ("gpt-5.3-codex", "GPT-5.3 Codex"),
        ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
        ("gpt-5.2", "GPT-5.2"),
    ]
    .into_iter()
    .map(|(id, name)| ModelInfo {
        id: id.to_string(),
        name: name.to_string(),
        provider_id: None,
        reasoning_options: xhigh_reasoning_options.clone(),
    })
    .collect::<Vec<_>>();
    models.splice(
        0..0,
        [
            ModelInfo {
                id: "gpt-5.6-sol".to_string(),
                name: "GPT-5.6-Sol".to_string(),
                provider_id: None,
                reasoning_options: ultra_reasoning_options.clone(),
            },
            ModelInfo {
                id: "gpt-5.6-terra".to_string(),
                name: "GPT-5.6-Terra".to_string(),
                provider_id: None,
                reasoning_options: ultra_reasoning_options,
            },
            ModelInfo {
                id: "gpt-5.6-luna".to_string(),
                name: "GPT-5.6-Luna".to_string(),
                provider_id: None,
                reasoning_options: max_reasoning_options,
            },
        ],
    );
    models
}

#[cfg(test)]
mod tests {
    #[test]
    fn goal_length_counts_unicode_characters_not_utf8_bytes() {
        assert!(super::validate_goal_objective(&"😀".repeat(4000)).is_ok());
        assert!(
            super::validate_goal_objective(&"あ".repeat(4001))
                .unwrap_err()
                .to_string()
                .contains("4001")
        );
    }
    use std::path::{Path, PathBuf};

    use codex_app_server_protocol::{Model, ModelListResponse, ReasoningEffortOption, UserInput};
    use codex_protocol::openai_models::ReasoningEffort as ProtocolReasoningEffort;
    use serde_json::json;

    use super::{
        AskForApproval, Codex, ReasoningEffort, ReasoningSummary, build_chat_input,
        fallback_models, model_list_response_to_model_infos, resolve_model, resume_params_from,
    };
    use crate::{
        actions::SelectedSkill,
        executors::{BaseCodingAgent, StandardCodingAgentExecutor},
        profile::{ExecutionMode, ExecutorConfig},
    };

    fn test_executor() -> Codex {
        serde_json::from_value(json!({})).expect("empty Codex config should deserialize")
    }

    fn config_with_reasoning(reasoning_id: &str) -> ExecutorConfig {
        ExecutorConfig {
            executor: BaseCodingAgent::Codex,
            variant: None,
            model_id: None,
            agent_id: None,
            reasoning_id: Some(reasoning_id.to_string()),
            permission_policy: None,
            execution_mode: None,
            goal_token_budget: None,
            goal_max_concurrent_agents: None,
        }
    }

    fn expected_memory_policy_args() -> Vec<String> {
        [
            "app-server",
            "-c",
            "features.memories=false",
            "-c",
            "memories.generate_memories=false",
            "-c",
            "memories.use_memories=false",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    #[test]
    fn app_server_uses_the_users_local_codex_command() {
        assert_eq!(Codex::base_command(), Codex::DEFAULT_BASE_COMMAND);
    }

    #[test]
    fn app_server_disables_native_memory_for_discovery_and_runtime() {
        let builder = test_executor()
            .build_command_builder()
            .expect("Codex app-server command should build");

        assert_eq!(builder.params, Some(expected_memory_policy_args()));
    }

    #[test]
    fn additional_params_can_explicitly_override_native_memory_policy() {
        let mut executor = test_executor();
        executor.cmd.additional_params = Some(vec![
            "-c".to_string(),
            "features.memories=true".to_string(),
            "-c".to_string(),
            "memories.generate_memories=true".to_string(),
            "-c".to_string(),
            "memories.use_memories=true".to_string(),
        ]);

        let params = executor
            .build_command_builder()
            .expect("Codex app-server command should build")
            .params
            .expect("Codex app-server command should have parameters");

        let expected_prefix = expected_memory_policy_args();
        assert_eq!(&params[..expected_prefix.len()], expected_prefix.as_slice());
        assert_eq!(
            &params[expected_prefix.len()..],
            [
                "-c",
                "features.memories=true",
                "-c",
                "memories.generate_memories=true",
                "-c",
                "memories.use_memories=true",
            ]
        );
    }

    #[test]
    fn resolve_model_detects_fast_suffix() {
        assert_eq!(resolve_model(Some("gpt-5.5-fast")), (Some("gpt-5.5"), true));
        assert_eq!(resolve_model(Some("gpt-5.4-fast")), (Some("gpt-5.4"), true));
    }

    #[test]
    fn resolve_model_leaves_non_fast_models_unchanged() {
        assert_eq!(resolve_model(Some("gpt-5.5")), (Some("gpt-5.5"), false));
        assert_eq!(
            resolve_model(Some("gpt-5.4-mini")),
            (Some("gpt-5.4-mini"), false)
        );
        assert_eq!(resolve_model(None), (None, false));
    }

    #[test]
    fn apply_overrides_preserves_codex_protocol_reasoning_values() {
        let mut executor = test_executor();
        executor.apply_overrides(&config_with_reasoning("minimal"));

        let overrides = executor
            .build_config_overrides()
            .expect("reasoning override should produce config");
        assert_eq!(
            overrides.get("model_reasoning_effort"),
            Some(&json!("minimal"))
        );
    }

    #[test]
    fn goal_overrides_are_scoped_to_the_app_server_thread() {
        let mut executor = test_executor();
        let mut config = ExecutorConfig::new(BaseCodingAgent::Codex);
        config.execution_mode = Some(ExecutionMode::Goal);
        config.goal_token_budget = Some(50_000);
        config.goal_max_concurrent_agents = Some(4);

        executor.apply_direct_overrides(&config);

        assert_eq!(executor.effective_execution_mode(), ExecutionMode::Goal);
        assert_eq!(executor.goal_token_budget, Some(50_000));
        let overrides = executor.build_config_overrides().unwrap();
        assert_eq!(overrides.get("agents.enabled"), Some(&json!(true)));
        assert_eq!(
            overrides.get("agents.max_concurrent_threads_per_session"),
            Some(&json!(4))
        );
    }

    #[test]
    fn parallel_agents_off_adds_config_and_objective_guardrails() {
        let mut executor = test_executor();
        let mut config = ExecutorConfig::new(BaseCodingAgent::Codex);
        config.execution_mode = Some(ExecutionMode::Goal);
        config.goal_max_concurrent_agents = Some(0);
        executor.apply_direct_overrides(&config);

        assert_eq!(
            executor
                .build_config_overrides()
                .unwrap()
                .get("agents.enabled"),
            Some(&json!(false))
        );
        assert!(
            executor
                .prepare_execution_prompt("Implement it".to_string())
                .contains("do not spawn or delegate to subagents")
        );
    }

    #[test]
    fn plan_with_goal_requests_an_approval_ready_goal_contract() {
        let mut executor = test_executor();
        let mut config = ExecutorConfig::new(BaseCodingAgent::Codex);
        config.execution_mode = Some(ExecutionMode::PlanWithGoal);
        executor.apply_direct_overrides(&config);

        let prompt = executor.prepare_execution_prompt("Implement it".to_string());
        assert!(prompt.contains("Outcome"));
        assert!(prompt.contains("Constraints"));
        assert!(prompt.contains("Verification and Completion Criteria"));
    }

    #[test]
    fn missing_reasoning_override_enables_safe_automatic_summaries() {
        let executor = test_executor();

        let overrides = executor
            .build_config_overrides()
            .expect("automatic summary should produce a config override");

        assert_eq!(overrides.get("model_reasoning_effort"), None);
        assert_eq!(
            overrides.get("model_reasoning_summary"),
            Some(&json!("auto"))
        );
    }

    #[test]
    fn explicit_none_disables_reasoning_summaries() {
        let mut executor = test_executor();
        executor.model_reasoning_summary = Some(ReasoningSummary::None);

        let overrides = executor
            .build_config_overrides()
            .expect("explicit summary setting should produce a config override");

        assert_eq!(
            overrides.get("model_reasoning_summary"),
            Some(&json!("none"))
        );
    }

    #[test]
    fn apply_overrides_preserves_custom_reasoning_values() {
        let mut executor = test_executor();
        executor.apply_overrides(&config_with_reasoning("max"));

        let overrides = executor
            .build_config_overrides()
            .expect("reasoning override should produce config");
        assert_eq!(overrides.get("model_reasoning_effort"), Some(&json!("max")));
    }

    #[test]
    fn reasoning_effort_maps_to_codex_protocol_values() {
        let xhigh = "xhigh"
            .parse::<ReasoningEffort>()
            .expect("xhigh is a valid reasoning effort");
        assert_eq!(xhigh.to_protocol(), ProtocolReasoningEffort::XHigh);

        let max = "max"
            .parse::<ReasoningEffort>()
            .expect("max is a valid reasoning effort");
        assert_eq!(max.to_protocol(), ProtocolReasoningEffort::Max);

        let ultra = "ultra"
            .parse::<ReasoningEffort>()
            .expect("ultra is a valid reasoning effort");
        assert_eq!(ultra.to_protocol(), ProtocolReasoningEffort::Ultra);

        let custom = "max-plus"
            .parse::<ReasoningEffort>()
            .expect("custom non-empty efforts are valid");
        assert_eq!(
            custom.to_protocol(),
            ProtocolReasoningEffort::Custom("max-plus".to_string())
        );
    }

    #[test]
    fn removed_on_failure_approval_policy_is_rejected() {
        assert!(serde_json::from_str::<AskForApproval>(r#""on-failure""#).is_err());
    }

    #[test]
    fn new_threads_use_supported_default_history_without_model_fallback() {
        let params = test_executor().build_thread_start_params(Path::new("/tmp/test-worktree"));

        assert!(params.history_mode.is_none());
        assert!(!params.allow_provider_model_fallback);
    }

    #[test]
    fn shared_resources_are_thread_scoped_and_preserved_on_resume() {
        use crate::env::{ExecutionEnv, RepoContext};

        let cwd = std::env::current_dir().unwrap();
        let shared = cwd.join("shared-test/persistent");
        let mut env = ExecutionEnv::new(RepoContext::default(), false, String::new());
        let mut executor = test_executor();
        assert!(
            executor
                .build_thread_start_params_with_resources(&cwd, &env)
                .runtime_workspace_roots
                .is_none()
        );
        env.insert(
            "EVK_SHARED_RESOURCE_ROOTS",
            serde_json::to_string(&vec![&shared]).unwrap(),
        );
        executor.sandbox = Some(super::SandboxMode::ReadOnly);
        let params = executor.build_thread_start_params_with_resources(&cwd, &env);
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["runtimeWorkspaceRoots"], json!([cwd, shared]));
        assert_eq!(json["sandbox"], "read-only");
        assert_eq!(
            params.developer_instructions.as_deref(),
            Some(crate::legacy_wiki::RETIREMENT_NOTICE)
        );
        let resume =
            serde_json::to_value(resume_params_from("thread-shared".into(), params)).unwrap();
        assert_eq!(
            resume["runtimeWorkspaceRoots"],
            json["runtimeWorkspaceRoots"]
        );
        assert_eq!(resume["sandbox"], "read-only");
    }

    #[test]
    fn resumed_threads_keep_the_same_id_and_start_overrides() {
        let start = test_executor().build_thread_start_params(Path::new("/tmp/test-worktree"));
        let expected_model = start.model.clone();
        let expected_cwd = start.cwd.clone();
        let expected_service_tier = start.service_tier.clone();

        let resume = resume_params_from("thread-1".to_string(), start);

        assert_eq!(resume.thread_id, "thread-1");
        assert_eq!(resume.model, expected_model);
        assert_eq!(resume.cwd, expected_cwd);
        assert_eq!(resume.service_tier, expected_service_tier);
        assert!(resume.path.is_none());
        assert!(resume.history.is_none());
    }

    #[test]
    fn model_list_uses_codex_default_reasoning_effort() {
        let response = ModelListResponse {
            data: vec![Model {
                id: "gpt-test".to_string(),
                model: "gpt-test".to_string(),
                upgrade: None,
                upgrade_info: None,
                availability_nux: None,
                display_name: "GPT Test".to_string(),
                description: "test model".to_string(),
                hidden: false,
                supported_reasoning_efforts: vec![
                    ReasoningEffortOption {
                        reasoning_effort: ProtocolReasoningEffort::High,
                        description: "High".to_string(),
                    },
                    ReasoningEffortOption {
                        reasoning_effort: ProtocolReasoningEffort::Minimal,
                        description: "Minimal".to_string(),
                    },
                ],
                default_reasoning_effort: ProtocolReasoningEffort::Minimal,
                input_modalities: vec![],
                supports_personality: false,
                additional_speed_tiers: vec![],
                service_tiers: vec![],
                default_service_tier: None,
                is_default: false,
            }],
            next_cursor: None,
        };

        let models = model_list_response_to_model_infos(response);

        assert_eq!(models.len(), 1);
        let reasoning_options = &models[0].reasoning_options;
        assert_eq!(
            reasoning_options
                .iter()
                .find(|option| option.is_default)
                .map(|option| option.id.as_str()),
            Some("minimal")
        );
    }

    #[test]
    fn model_list_preserves_gpt_5_6_max_and_ultra_reasoning_efforts() {
        let response = ModelListResponse {
            data: vec![Model {
                id: "gpt-5.6-sol".to_string(),
                model: "gpt-5.6-sol".to_string(),
                upgrade: None,
                upgrade_info: None,
                availability_nux: None,
                display_name: "GPT-5.6-Sol".to_string(),
                description: "Latest frontier agentic coding model.".to_string(),
                hidden: false,
                supported_reasoning_efforts: vec![
                    ReasoningEffortOption {
                        reasoning_effort: ProtocolReasoningEffort::Max,
                        description: "Maximum reasoning depth".to_string(),
                    },
                    ReasoningEffortOption {
                        reasoning_effort: ProtocolReasoningEffort::Ultra,
                        description: "Maximum reasoning with delegation".to_string(),
                    },
                ],
                default_reasoning_effort: ProtocolReasoningEffort::Max,
                input_modalities: vec![],
                supports_personality: false,
                additional_speed_tiers: vec![],
                service_tiers: vec![],
                default_service_tier: None,
                is_default: true,
            }],
            next_cursor: None,
        };

        let models = model_list_response_to_model_infos(response);
        let ids = models[0]
            .reasoning_options
            .iter()
            .map(|option| option.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["max", "ultra"]);
        assert!(models[0].reasoning_options[0].is_default);
    }

    #[test]
    fn fallback_models_include_gpt_5_6_catalog() {
        let models = fallback_models();

        assert_eq!(
            models
                .iter()
                .take(3)
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]
        );
        let sol = &models[0];
        assert!(
            sol.reasoning_options
                .iter()
                .any(|option| option.id == "max")
        );
        assert!(
            sol.reasoning_options
                .iter()
                .any(|option| option.id == "ultra")
        );
    }

    #[test]
    fn build_chat_input_places_selected_skills_before_text() {
        let skill_path = PathBuf::from("/tmp/skills/review/SKILL.md");
        let input = build_chat_input(
            "review this change".to_string(),
            vec![SelectedSkill {
                name: "code-review".to_string(),
                path: skill_path.clone(),
            }],
        );

        assert_eq!(input.len(), 2);
        match &input[0] {
            UserInput::Skill { name, path } => {
                assert_eq!(name, "code-review");
                assert_eq!(path, &skill_path);
            }
            other => panic!("expected skill input first, got {other:?}"),
        }
        match &input[1] {
            UserInput::Text {
                text,
                text_elements,
            } => {
                assert_eq!(text, "review this change");
                assert!(text_elements.is_empty());
            }
            other => panic!("expected text input second, got {other:?}"),
        }
    }

    #[test]
    fn build_chat_input_does_not_augment_retired_wiki_and_filters_owned_skills() {
        let prompt = "task\n<!-- vk:pipeline:start -->\n## Pipeline: LLM Wiki\n1. recall\n<!-- vk:pipeline:end -->";
        let input = build_chat_input(
            prompt.to_string(),
            vec![SelectedSkill {
                name: "knowledge-recall".into(),
                path: workspace_utils::assets::asset_dir()
                    .join("skills/llm-wiki/knowledge-recall/SKILL.md"),
            }],
        );
        assert_eq!(input.len(), 1);
        assert!(matches!(&input[0], UserInput::Text { text, .. } if text == prompt));
    }
}

#[cfg(test)]
mod goal_skill_tests {
    use super::*;

    #[test]
    fn bootstrap_reviewer_overrides_writer_profile_without_shared_config_writes() {
        let codex: Codex =
            serde_json::from_value(serde_json::json!({"sandbox":"danger-full-access"})).unwrap();
        let mut env = ExecutionEnv::new(RepoContext::default(), false, String::new());
        env.insert("EVK_OPENWIKI_REVIEWER", "1");
        env.insert("EVK_OPENWIKI_MAINTENANCE", "1");
        env.insert(
            "EVK_REPOSITORY_MEMORY_INSTRUCTIONS",
            "previous task memory must not leak",
        );
        env.insert("EVK_SHARED_RESOURCE_ROOTS", r#"["/shared/writable"]"#);
        let params = codex.build_thread_start_params_with_resources(Path::new("/workspace"), &env);
        assert_eq!(params.sandbox, Some(V2SandboxMode::ReadOnly));
        assert_eq!(params.approval_policy, Some(V2AskForApproval::Never));
        assert!(params.runtime_workspace_roots.is_none());
        assert!(
            !params
                .developer_instructions
                .unwrap_or_default()
                .contains("previous task")
        );
        let config = params.config.unwrap();
        assert_eq!(config["mcp_servers.openwiki.enabled"], false);
        assert!(!config.contains_key("mcp_servers.openwiki.command"));
        assert_eq!(
            config["skills.config"],
            serde_json::json!([{"name":"openwiki","enabled":false}])
        );
    }

    #[test]
    fn repository_memory_is_persistent_for_initial_and_resumed_threads() {
        let codex: Codex = serde_json::from_value(serde_json::json!({})).unwrap();
        let mut env = ExecutionEnv::new(RepoContext::default(), false, String::new());
        env.insert("EVK_REPOSITORY_MEMORY_INSTRUCTIONS", "Read the workspace-specific memory after context compaction. Canonical openwiki/ is read-only.");
        let params = codex.build_thread_start_params_with_resources(Path::new("/workspace"), &env);
        assert!(
            params
                .developer_instructions
                .as_deref()
                .unwrap()
                .contains("after context compaction")
        );
        assert!(
            params
                .developer_instructions
                .as_deref()
                .unwrap()
                .contains(crate::legacy_wiki::RETIREMENT_NOTICE)
        );
        assert_eq!(
            params.config.as_ref().unwrap()["mcp_servers.openwiki.enabled"],
            false
        );
        let resumed = resume_params_from("existing".into(), params.clone());
        assert_eq!(
            params.developer_instructions,
            resumed.developer_instructions
        );
        assert!(
            !params
                .config
                .as_ref()
                .is_some_and(|config| config.contains_key("mcp_servers.openwiki.command"))
        );
        env.insert("EVK_OPENWIKI_MAINTENANCE", "1");
        let maintenance =
            codex.build_thread_start_params_with_resources(Path::new("/workspace"), &env);
        assert_eq!(
            maintenance.config.unwrap()["mcp_servers.openwiki.args"],
            serde_json::json!(["mcp", "--host", "codex"])
        );
    }

    #[test]
    fn goal_skills_are_available_before_initial_or_resumed_activation() {
        let mut params = ThreadStartParams {
            developer_instructions: Some("existing rules".into()),
            ..Default::default()
        };
        let selected = vec![SelectedSkill {
            name: "knowledge-recall".into(),
            path: PathBuf::from("/skills/knowledge-recall/SKILL.md"),
        }];
        add_goal_skill_context(&mut params, &selected);
        let instructions = params.developer_instructions.as_deref().unwrap();
        assert!(instructions.starts_with("existing rules"));
        assert!(instructions.contains("knowledge-recall/SKILL.md"));
        assert!(instructions.contains("does not require invoking"));
        let resumed = resume_params_from("existing-thread".into(), params.clone());
        assert_eq!(
            params.developer_instructions,
            resumed.developer_instructions
        );
        let mut none = ThreadStartParams::default();
        add_goal_skill_context(&mut none, &[]);
        assert!(none.developer_instructions.is_none());
    }
}
