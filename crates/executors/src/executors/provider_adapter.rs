//! Provider-neutral entry points for the four direct V1 runtime adapters.
//!
//! Legacy product paths may still use `StandardCodingAgentExecutor`, while the
//! V1 runtime launches the same concrete providers through the narrow API in
//! this module. Native bytes are decoded once here, then mapped to canonical
//! events. Consumers must not parse provider output themselves.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    actions::SelectedSkill,
    approvals::ExecutorApprovalService,
    env::ExecutionEnv,
    executors::{CodingAgent, ExecutorError, SpawnedChild},
    profile::{ExecutorConfig, ExecutorConfigs},
    runtime::{
        AGENT_EVENT_PAYLOAD_VERSION, AGENT_EVENT_SCHEMA_VERSION, AGENT_LIVE_EVENT_SCHEMA_VERSION,
        AgentEvent, AgentEventEnvelope, AgentEventPayload, AgentLiveEvent, AgentLiveEventPayload,
        AgentRunStatus, AgentRuntimeError, AgentRuntimeErrorKind, AgentRuntimeMessageRole,
        AgentRuntimeToolStatus, AgentTransportKind, CapabilitySnapshot, CapabilitySnapshotEntry,
        CapabilitySource, CapabilityState, NativeAuditError, NativeAuditFrame, NativeAuditManifest,
        NativeAuditReplayMapper, NativeAuditVersionSet, ProviderEvent, ProviderSessionReference,
    },
};

/// Everything needed to launch one real provider process without constructing
/// a legacy `ExecutorAction` or routing through `StandardCodingAgentExecutor`.
pub struct DirectProviderLaunchRequest<'a> {
    pub provider: DirectProvider,
    pub executor_config: &'a ExecutorConfig,
    pub intent: DirectIntent,
    pub prompt: &'a str,
    pub provider_session: Option<&'a ProviderSessionReference>,
    pub reset_to_message_id: Option<&'a str>,
    pub selected_skills: &'a [SelectedSkill],
    pub approvals: Arc<dyn ExecutorApprovalService>,
    pub current_dir: &'a Path,
    pub env: &'a ExecutionEnv,
}

/// Resolve the requested profile and launch the provider through its concrete
/// adapter. Profile overrides, approvals, reviews, and Codex selected skills
/// retain the same behavior as the existing product launch path.
pub async fn launch_direct_provider(
    request: DirectProviderLaunchRequest<'_>,
) -> Result<SpawnedChild, ExecutorError> {
    validate_direct_launch(&request)?;

    let profile_id = request.executor_config.profile_id();
    let mut agent = ExecutorConfigs::get_cached()
        .get_coding_agent(&profile_id)
        .ok_or_else(|| ExecutorError::UnknownExecutorType(profile_id.to_string()))?;
    if DirectProvider::from_agent(&agent) != Some(request.provider) {
        return Err(profile_provider_mismatch(
            request.provider,
            request.executor_config,
        ));
    }

    match &mut agent {
        CodingAgent::Gemini(agent) => {
            agent.apply_direct_overrides(request.executor_config);
            agent.use_direct_approvals(request.approvals);
            agent
                .launch_direct(
                    request.intent,
                    request.current_dir,
                    request.prompt,
                    provider_session_id(request.provider_session),
                    request.env,
                )
                .await
        }
        CodingAgent::Codex(agent) => {
            agent.apply_direct_overrides(request.executor_config);
            agent.use_direct_approvals(request.approvals);
            agent
                .launch_direct(
                    request.intent,
                    request.current_dir,
                    request.prompt,
                    provider_session_id(request.provider_session),
                    request.selected_skills,
                    request.env,
                )
                .await
        }
        CodingAgent::ClaudeCode(agent) => {
            agent.apply_direct_overrides(request.executor_config);
            agent.use_direct_approvals(request.approvals);
            agent
                .launch_direct(
                    request.intent,
                    request.current_dir,
                    request.prompt,
                    provider_session_id(request.provider_session),
                    request.reset_to_message_id,
                    request.env,
                )
                .await
        }
        CodingAgent::OhMyPi(agent) => {
            agent.apply_direct_overrides(request.executor_config);
            agent
                .launch_direct(
                    request.intent,
                    request.current_dir,
                    request.prompt,
                    provider_session_id(request.provider_session),
                    request.env,
                )
                .await
        }
        #[cfg(feature = "qa-mode")]
        CodingAgent::QaMock(_) => Err(profile_provider_mismatch(
            request.provider,
            request.executor_config,
        )),
    }
}

fn validate_direct_launch(request: &DirectProviderLaunchRequest<'_>) -> Result<(), ExecutorError> {
    let configured_provider = DirectProvider::from_base_agent(request.executor_config.executor)
        .ok_or_else(|| profile_provider_mismatch(request.provider, request.executor_config))?;
    if configured_provider != request.provider {
        return Err(profile_provider_mismatch(
            request.provider,
            request.executor_config,
        ));
    }

    let profile_id = request.executor_config.profile_id().cache_key();
    if let Some(session) = request.provider_session {
        session.validate_current().map_err(|error| {
            ExecutorError::FollowUpNotSupported(format!(
                "invalid provider session for {}: {error}",
                request.provider.id()
            ))
        })?;
        if session.provider_id != request.provider.id() || session.runtime_profile_id != profile_id
        {
            return Err(ExecutorError::FollowUpNotSupported(format!(
                "provider session does not match {} profile {profile_id}",
                request.provider.id()
            )));
        }
        validate_native_session_id(request.provider, &session.provider_session_id).map_err(
            |error| {
                ExecutorError::FollowUpNotSupported(format!(
                    "invalid provider session for {}: {error}",
                    request.provider.id()
                ))
            },
        )?;
    }

    match request.intent {
        DirectIntent::Initial if request.provider_session.is_some() => {
            return Err(ExecutorError::FollowUpNotSupported(
                "initial launch cannot attach a provider session".to_string(),
            ));
        }
        DirectIntent::FollowUp | DirectIntent::Resume if request.provider_session.is_none() => {
            return Err(ExecutorError::FollowUpNotSupported(format!(
                "{} requires an explicit provider session",
                request.provider.id()
            )));
        }
        _ => {}
    }

    if request.reset_to_message_id.is_some()
        && (request.provider != DirectProvider::ClaudeCode
            || !matches!(
                request.intent,
                DirectIntent::FollowUp | DirectIntent::Resume
            ))
    {
        return Err(ExecutorError::ResetToMessageNotSupported(format!(
            "{} does not support message-level reset for {:?}",
            request.provider.id(),
            request.intent
        )));
    }

    Ok(())
}

fn provider_session_id(session: Option<&ProviderSessionReference>) -> Option<&str> {
    session.map(|session| session.provider_session_id.as_str())
}

fn profile_provider_mismatch(
    provider: DirectProvider,
    executor_config: &ExecutorConfig,
) -> ExecutorError {
    ExecutorError::UnknownExecutorType(format!(
        "direct provider {} does not match executor profile {}",
        provider.id(),
        executor_config.profile_id()
    ))
}

/// Validate an opaque provider-native session identifier before it is persisted
/// or handed to a provider process. Session IDs are intentionally not parsed
/// as UUIDs (providers choose their own format), but control characters and
/// option-like values would be ambiguous on the wire or alter CLI parsing.
/// Provider adapters remain authoritative for the actual existence check.
pub fn validate_native_session_id(
    provider: DirectProvider,
    session_id: &str,
) -> Result<(), String> {
    let trimmed = session_id.trim();
    if trimmed != session_id {
        return Err("native session id cannot have leading or trailing whitespace".to_string());
    }
    let session_id = trimmed;
    if session_id.is_empty() {
        return Err("native session id cannot be blank".to_string());
    }
    if session_id.chars().any(char::is_control) {
        return Err("native session id contains control characters".to_string());
    }
    if matches!(provider, DirectProvider::Gemini | DirectProvider::OhMyPi)
        && session_id.starts_with('-')
    {
        return Err(format!(
            "{} native session id cannot start with '-'",
            provider.id()
        ));
    }
    Ok(())
}

/// The only provider products included in the V1 adapter gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectProvider {
    Gemini,
    Codex,
    ClaudeCode,
    OhMyPi,
}

impl DirectProvider {
    pub const ALL: [Self; 4] = [Self::Gemini, Self::Codex, Self::ClaudeCode, Self::OhMyPi];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude_code",
            Self::OhMyPi => "oh_my_pi",
        }
    }

    /// Construct this adapter's native Tool Manager. Provider path and
    /// encoding decisions remain owned by the direct adapter boundary.
    pub fn tool_manager(
        self,
        home_dir: PathBuf,
        project_path: Option<PathBuf>,
        disabled_root: PathBuf,
    ) -> crate::agent_tools::ProviderToolManager {
        crate::agent_tools::ProviderToolManager::new(
            self.into(),
            home_dir,
            project_path,
            disabled_root,
        )
    }

    /// Construct this adapter's native Settings Manager. Provider paths,
    /// schema descriptors, precedence, and native encodings stay behind the
    /// direct-provider boundary.
    pub fn settings_manager(
        self,
        home_dir: PathBuf,
        project_path: Option<PathBuf>,
    ) -> crate::agent_settings::ProviderSettingsManager {
        crate::agent_settings::ProviderSettingsManager::new(self.into(), home_dir, project_path)
    }

    pub const fn from_base_agent(agent: crate::executors::BaseCodingAgent) -> Option<Self> {
        match agent {
            crate::executors::BaseCodingAgent::Gemini => Some(Self::Gemini),
            crate::executors::BaseCodingAgent::Codex => Some(Self::Codex),
            crate::executors::BaseCodingAgent::ClaudeCode => Some(Self::ClaudeCode),
            crate::executors::BaseCodingAgent::OhMyPi => Some(Self::OhMyPi),
            #[cfg(feature = "qa-mode")]
            crate::executors::BaseCodingAgent::QaMock => None,
        }
    }

    pub fn from_agent(agent: &CodingAgent) -> Option<Self> {
        Self::from_base_agent(crate::executors::BaseCodingAgent::from(agent))
    }

    pub const fn transport(self) -> AgentTransportKind {
        match self {
            Self::Gemini => AgentTransportKind::Acp,
            Self::Codex => AgentTransportKind::AppServerJsonrpc,
            Self::ClaudeCode => AgentTransportKind::StdioRpc,
            Self::OhMyPi => AgentTransportKind::StdioRpc,
        }
    }

    pub const fn versions(self) -> DirectAdapterVersions {
        match self {
            Self::Gemini => DirectAdapterVersions {
                executable: "gemini",
                runtime: None,
                protocol: Some("acp-0.8"),
                adapter: "gemini-adapter-v1",
                mapper: "gemini-mapper-v2",
            },
            Self::Codex => DirectAdapterVersions {
                executable: "codex",
                // The runtime version is reported by app-server initialize;
                // static adapter metadata must not claim a required release.
                runtime: None,
                protocol: Some("rust-v0.144.1"),
                adapter: "codex-adapter-v1",
                mapper: "codex-mapper-v3",
            },
            Self::ClaudeCode => DirectAdapterVersions {
                executable: "claude",
                runtime: None,
                protocol: Some("stream-json-v1"),
                adapter: "claude-code-adapter-v1",
                mapper: "claude-code-mapper-v2",
            },
            Self::OhMyPi => DirectAdapterVersions {
                executable: "omp",
                runtime: None,
                protocol: Some("stdio-rpc-ndjson-v1"),
                adapter: "oh-my-pi-adapter-v1",
                mapper: "oh-my-pi-mapper-v2",
            },
        }
    }

    pub fn version_set(self) -> NativeAuditVersionSet {
        let versions = self.versions();
        NativeAuditVersionSet {
            audit_schema_version: crate::runtime::NATIVE_AUDIT_SCHEMA_VERSION,
            runtime_version: versions.runtime.map(str::to_owned),
            adapter_version: versions.adapter.to_owned(),
            protocol_version: versions.protocol.map(str::to_owned),
            mapper_version: versions.mapper.to_owned(),
        }
    }

    pub fn capabilities(self, runtime_profile_id: impl Into<String>) -> CapabilitySnapshot {
        let versions = self.versions();
        let states = match self {
            Self::Gemini => capability_states(&[
                (
                    crate::runtime::AgentCapability::SessionResume,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Steering,
                    CapabilityState::Unsupported,
                ),
                (
                    crate::runtime::AgentCapability::Approval,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Images,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Review,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Mcp,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Subagents,
                    CapabilityState::Unknown,
                ),
                (
                    crate::runtime::AgentCapability::TokenUsage,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Goal,
                    CapabilityState::Unsupported,
                ),
            ]),
            Self::Codex => capability_states(&[
                (
                    crate::runtime::AgentCapability::SessionResume,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Steering,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Approval,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Images,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Review,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Mcp,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Subagents,
                    CapabilityState::Unknown,
                ),
                (
                    crate::runtime::AgentCapability::TokenUsage,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Goal,
                    CapabilityState::Native,
                ),
            ]),
            Self::ClaudeCode => capability_states(&[
                (
                    crate::runtime::AgentCapability::SessionResume,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Steering,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Approval,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Images,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Review,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Mcp,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Subagents,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::TokenUsage,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Goal,
                    CapabilityState::Unsupported,
                ),
            ]),
            Self::OhMyPi => capability_states(&[
                (
                    crate::runtime::AgentCapability::SessionResume,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Steering,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Approval,
                    CapabilityState::Unsupported,
                ),
                (
                    crate::runtime::AgentCapability::Images,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Review,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Mcp,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Subagents,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::TokenUsage,
                    CapabilityState::Native,
                ),
                (
                    crate::runtime::AgentCapability::Goal,
                    CapabilityState::Unsupported,
                ),
            ]),
        };
        CapabilitySnapshot {
            schema_version: crate::runtime::CAPABILITY_SNAPSHOT_SCHEMA_VERSION,
            runtime_profile_id: runtime_profile_id.into(),
            provider_id: self.id().to_string(),
            runtime_version: versions.runtime.map(str::to_owned),
            protocol_version: versions.protocol.map(str::to_owned),
            adapter_version: versions.adapter.to_string(),
            resolved_at: Utc::now(),
            capabilities: states,
        }
    }

    pub fn mapper(self) -> DirectProviderMapper {
        DirectProviderMapper {
            provider: self,
            semantics: if self == Self::Codex {
                MapperSemantics::V3
            } else {
                MapperSemantics::V2
            },
            scope: Default::default(),
        }
    }

    pub fn semantic_mapper(self) -> DirectProviderMapper {
        DirectProviderMapper {
            provider: self,
            semantics: MapperSemantics::V2,
            scope: Default::default(),
        }
    }

    /// Retains deterministic replay for Native Audit bundles written before
    /// semantic compaction was introduced. New runtime streams always use v2.
    pub fn legacy_mapper(self) -> DirectProviderMapper {
        DirectProviderMapper {
            provider: self,
            semantics: MapperSemantics::V1,
            scope: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectAdapterVersions {
    pub executable: &'static str,
    pub runtime: Option<&'static str>,
    pub protocol: Option<&'static str>,
    pub adapter: &'static str,
    pub mapper: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectIntent {
    Initial,
    FollowUp,
    Review,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DirectControl {
    Cancel,
    Approve {
        request_id: String,
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Input {
        request_id: String,
        text: String,
    },
    Steer {
        text: String,
    },
    UpdatePlanGoalDraft {
        objective: String,
    },
    GoalUpdate {
        objective: Option<String>,
        status: Option<crate::runtime::AgentGoalStatus>,
        token_budget: Option<Option<i64>>,
    },
    GoalClear,
}

impl DirectControl {
    fn capability(&self) -> Option<crate::runtime::AgentCapability> {
        match self {
            Self::Cancel => None,
            Self::Approve { .. } => Some(crate::runtime::AgentCapability::Approval),
            Self::Input { .. } => None,
            Self::Steer { .. } => Some(crate::runtime::AgentCapability::Steering),
            Self::UpdatePlanGoalDraft { .. } => Some(crate::runtime::AgentCapability::Goal),
            Self::GoalUpdate { .. } | Self::GoalClear => {
                Some(crate::runtime::AgentCapability::Goal)
            }
        }
    }
}

/// Encode one provider-native control frame after checking the frozen attempt
/// capability snapshot.  Unsupported/unknown capabilities never fall through
/// to another control shape.
pub fn encode_control(
    provider: DirectProvider,
    snapshot: &CapabilitySnapshot,
    control: DirectControl,
) -> Result<Vec<u8>, DirectControlError> {
    if let Some(capability) = control.capability() {
        require_capability(provider, snapshot, capability, false)?;
    }
    match provider {
        DirectProvider::Gemini => super::gemini::command_adapter::encode_control(control),
        DirectProvider::Codex => super::codex::command_adapter::encode_control(control),
        DirectProvider::ClaudeCode => super::claude::command_adapter::encode_control(control),
        DirectProvider::OhMyPi => super::oh_my_pi::command_adapter::encode_control(control),
    }
    .map_err(DirectControlError::Json)
}

#[derive(Debug, thiserror::Error)]
pub enum DirectControlError {
    #[error(transparent)]
    Capability(#[from] CapabilityGateError),
    #[error(transparent)]
    Json(serde_json::Error),
}

fn capability_states(
    values: &[(crate::runtime::AgentCapability, CapabilityState)],
) -> Vec<CapabilitySnapshotEntry> {
    values
        .iter()
        .map(|(capability, state)| CapabilitySnapshotEntry {
            capability: *capability,
            state: *state,
            source: CapabilitySource::RuntimeProfile,
            emulation_policy: None,
            evidence: Some(serde_json::json!({ "provider_profile": "v1" })),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CapabilityGateError {
    #[error("capability {capability:?} is unresolved for {provider}")]
    Unresolved {
        provider: String,
        capability: crate::runtime::AgentCapability,
    },
    #[error("capability {capability:?} is unsupported for {provider}")]
    Unsupported {
        provider: String,
        capability: crate::runtime::AgentCapability,
    },
    #[error("emulated capability {capability:?} requires explicit policy approval")]
    EmulationNotAllowed {
        capability: crate::runtime::AgentCapability,
    },
}

pub fn require_capability(
    provider: DirectProvider,
    snapshot: &CapabilitySnapshot,
    capability: crate::runtime::AgentCapability,
    allow_emulated: bool,
) -> Result<(), CapabilityGateError> {
    match snapshot.resolve(capability) {
        CapabilityState::Native => Ok(()),
        CapabilityState::Emulated if allow_emulated => Ok(()),
        CapabilityState::Emulated => Err(CapabilityGateError::EmulationNotAllowed { capability }),
        CapabilityState::Unsupported => Err(CapabilityGateError::Unsupported {
            provider: provider.id().to_string(),
            capability,
        }),
        CapabilityState::Unknown => Err(CapabilityGateError::Unresolved {
            provider: provider.id().to_string(),
            capability,
        }),
    }
}

/// Typed provider semantics produced by the one native-frame decode.
#[derive(Debug, Clone, PartialEq)]
pub enum TypedProviderEvent {
    AgentActivity(crate::runtime::AgentActivity),
    Lifecycle(AgentRunStatus),
    SessionObserved(String),
    Message {
        provider_message_id: Option<String>,
        role: AgentRuntimeMessageRole,
        content: String,
        final_output: bool,
    },
    MessageDelta {
        provider_message_id: String,
        role: AgentRuntimeMessageRole,
        delta: String,
    },
    ThinkingDelta {
        provider_item_id: String,
        delta: String,
    },
    ToolOutputDelta {
        provider_item_id: String,
        delta: String,
    },
    Thinking(String),
    ToolCall {
        id: Option<String>,
        name: String,
        status: AgentRuntimeToolStatus,
        arguments: Option<Value>,
        result: Option<Value>,
    },
    ApprovalRequested {
        id: String,
        tool_call_id: Option<String>,
        tool_name: String,
    },
    ApprovalResolved {
        id: String,
        approved: bool,
        reason: Option<String>,
    },
    InputRequested {
        id: String,
        prompt: String,
    },
    InputResolved {
        id: String,
        answered: bool,
    },
    TokenUsage {
        input_tokens: u64,
        output_tokens: u64,
        cached_input_tokens: Option<u64>,
    },
    GoalUpdated(crate::runtime::AgentGoalState),
    GoalCleared,
    Error(AgentRuntimeError),
    Unknown {
        event_type: String,
        payload: Value,
    },
    AuditOnly {
        event_type: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEventClass {
    DurableSemantic,
    LiveOnly,
    SnapshotOnly,
    AuditOnly,
}

impl TypedProviderEvent {
    pub fn class(&self) -> ProviderEventClass {
        match self {
            Self::MessageDelta { .. }
            | Self::ThinkingDelta { .. }
            | Self::ToolOutputDelta { .. } => ProviderEventClass::LiveOnly,
            Self::TokenUsage { .. } => ProviderEventClass::SnapshotOnly,
            Self::AuditOnly { .. } | Self::Unknown { .. } => ProviderEventClass::AuditOnly,
            _ => ProviderEventClass::DurableSemantic,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEventProjection {
    pub durable_events: Vec<AgentEvent>,
    pub live_events: Vec<AgentLiveEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedProviderEvent {
    pub raw: ProviderEvent,
    pub typed: TypedProviderEvent,
}

/// Per-stream provider scoping. Common runtime callers do not inspect native
/// thread identifiers or provider-specific notification names.
pub struct ProviderStreamScope {
    provider: DirectProvider,
    codex: super::codex::agent_scope::AgentScope,
}

impl ProviderStreamScope {
    pub fn new(provider: DirectProvider) -> Self {
        Self {
            provider,
            codex: Default::default(),
        }
    }
    pub fn apply(&mut self, event: &mut DecodedProviderEvent) {
        if self.provider == DirectProvider::Codex {
            self.codex.apply(event);
        }
    }
}

#[derive(Debug)]
pub enum ProviderFrameClassification {
    Event {
        class: ProviderEventClass,
        event: Box<DecodedProviderEvent>,
    },
    UnsupportedRequired {
        error: NativeAuditError,
    },
}

impl DirectProvider {
    pub fn classify_native_frame(self, frame: &NativeAuditFrame) -> ProviderFrameClassification {
        match self.decode_native_frame(frame) {
            Ok(event) => ProviderFrameClassification::Event {
                class: event.typed.class(),
                event: Box::new(event),
            },
            Err(error) => ProviderFrameClassification::UnsupportedRequired { error },
        }
    }

    pub fn decode_native_frame(
        self,
        frame: &NativeAuditFrame,
    ) -> Result<DecodedProviderEvent, NativeAuditError> {
        let raw = crate::runtime::DefaultNativeAuditMapper::for_manifest(&fixture_manifest(self))
            .decode(frame)?;
        let typed = classify_payload(self, &raw)?;
        Ok(DecodedProviderEvent { raw, typed })
    }

    pub fn map_provider_event(
        self,
        event: &DecodedProviderEvent,
        manifest: &NativeAuditManifest,
    ) -> Result<Vec<AgentEvent>, NativeAuditError> {
        Ok(project_typed_event(self, event, manifest)?.durable_events)
    }

    pub fn project_provider_event(
        self,
        event: &DecodedProviderEvent,
        manifest: &NativeAuditManifest,
    ) -> Result<ProviderEventProjection, NativeAuditError> {
        project_typed_event(self, event, manifest)
    }
}

/// Replay mapper used by `AuditBundle::replay`; all four adapters share the
/// canonical envelope construction while retaining provider-specific names.
#[derive(Debug)]
pub struct DirectProviderMapper {
    pub provider: DirectProvider,
    semantics: MapperSemantics,
    scope: std::sync::Mutex<super::codex::agent_scope::AgentScope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapperSemantics {
    V1,
    V2,
    V3,
}

impl NativeAuditReplayMapper for DirectProviderMapper {
    fn versions(&self) -> NativeAuditVersionSet {
        let mut versions = self.provider.version_set();
        let suffix = match self.semantics {
            MapperSemantics::V1 => "v1",
            MapperSemantics::V2 => "v2",
            MapperSemantics::V3 => "v3",
        };
        if let Some((prefix, _)) = versions.mapper_version.rsplit_once('-') {
            versions.mapper_version = format!("{prefix}-{suffix}");
        }
        versions
    }

    fn decode(&self, frame: &NativeAuditFrame) -> Result<ProviderEvent, NativeAuditError> {
        Ok(self.provider.decode_native_frame(frame)?.raw)
    }

    fn map(
        &self,
        event: &ProviderEvent,
        manifest: &NativeAuditManifest,
    ) -> Result<Vec<AgentEvent>, NativeAuditError> {
        let typed = classify_payload(self.provider, event)?;
        let mut decoded = DecodedProviderEvent {
            raw: event.clone(),
            typed,
        };
        match self.semantics {
            MapperSemantics::V1 => map_typed_event_v1(self.provider, &decoded, manifest),
            MapperSemantics::V2 => {
                Ok(project_typed_event(self.provider, &decoded, manifest)?.durable_events)
            }
            MapperSemantics::V3 => {
                let mut scope = self.scope.lock().expect("replay scope lock");
                if event.sequence == 1 {
                    *scope = Default::default();
                }
                scope.apply(&mut decoded);
                Ok(project_typed_event(self.provider, &decoded, manifest)?.durable_events)
            }
        }
    }
}

fn fixture_manifest(provider: DirectProvider) -> NativeAuditManifest {
    // Only used to invoke the existing lossless frame decoder.  The returned
    // ProviderEvent's native reference is replaced with the real manifest by
    // AuditBundle::replay.
    let now = Utc::now();
    NativeAuditManifest {
        audit_schema_version: crate::runtime::NATIVE_AUDIT_SCHEMA_VERSION,
        session_id: Uuid::nil(),
        agent_run_id: Uuid::nil(),
        turn_id: Uuid::nil(),
        run_attempt_id: Uuid::nil(),
        run_attempt_number: 1,
        provider_id: provider.id().to_string(),
        runtime_profile_id: "fixture".to_string(),
        workspace_path: String::new(),
        runtime_version: provider.versions().runtime.map(str::to_owned),
        protocol_version: provider.versions().protocol.map(str::to_owned),
        adapter_version: provider.versions().adapter.to_string(),
        mapper_version: provider.versions().mapper.to_string(),
        frame_count: 0,
        first_sequence: None,
        last_sequence: None,
        final_checksum: None,
        integrity_status: crate::runtime::NativeAuditIntegrityStatus::Open,
        created_at: now,
        closed_at: None,
        manifest_relative_path: String::new(),
        frames_relative_path: String::new(),
        raw_content_trusted: true,
    }
}

fn classify_payload(
    provider: DirectProvider,
    event: &ProviderEvent,
) -> Result<TypedProviderEvent, NativeAuditError> {
    let Some(payload) = event.payload_json.as_ref() else {
        let text = String::from_utf8_lossy(&event.payload).trim().to_string();
        if text.is_empty() {
            return Ok(TypedProviderEvent::Unknown {
                event_type: "empty".to_string(),
                payload: Value::Null,
            });
        }
        return Ok(TypedProviderEvent::Message {
            provider_message_id: None,
            role: AgentRuntimeMessageRole::Assistant,
            content: text,
            final_output: false,
        });
    };

    if provider == DirectProvider::Codex
        && payload.get("id").is_some()
        && let Some(message) = payload.pointer("/error/message").and_then(Value::as_str)
    {
        return Ok(TypedProviderEvent::Error(
            AgentRuntimeError::new(AgentRuntimeErrorKind::Unknown, message)
                .with_provider(Some(provider.id())),
        ));
    }

    if provider == DirectProvider::Codex
        && let Some(message) = payload
            .pointer("/LaunchError/error")
            .and_then(Value::as_str)
    {
        return Ok(TypedProviderEvent::Error(
            AgentRuntimeError::new(
                AgentRuntimeErrorKind::Unknown,
                message
                    .split("\n\nCodex launch context:")
                    .next()
                    .unwrap_or(message),
            )
            .with_provider(Some(provider.id())),
        ));
    }

    if provider == DirectProvider::Codex
        && let Some(classified) = classify_codex_payload(payload, event.sequence)
    {
        return classified;
    }

    let object = payload.as_object();
    let event_type = object
        .and_then(|map| map.get("type").or_else(|| map.get("method")))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    let value = |keys: &[&str]| -> Option<&Value> {
        object.and_then(|map| keys.iter().find_map(|key| map.get(*key)))
    };
    let text = || {
        value(&["text", "content", "message", "delta", "output"])
            .and_then(Value::as_str)
            .map(str::to_owned)
    };

    match event_type.as_str() {
        "init" | "initialize" | "session" | "session_started" | "thread_started"
        | "session_observed" => value(&["session_id", "sessionId", "thread_id", "threadId"])
            .and_then(Value::as_str)
            .map(|id| TypedProviderEvent::SessionObserved(id.to_string()))
            .ok_or(NativeAuditError::MalformedFrame(event.sequence)),
        "thinking" | "reasoning" | "thought" => {
            Ok(TypedProviderEvent::Thinking(text().unwrap_or_default()))
        }
        "assistant" | "message" | "text" | "content" | "delta" | "assistant_message"
        | "text_delta" => Ok(TypedProviderEvent::Message {
            provider_message_id: None,
            role: AgentRuntimeMessageRole::Assistant,
            content: text().unwrap_or_default(),
            final_output: value(&["final", "is_final", "done"])
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "user" | "user_message" => Ok(TypedProviderEvent::Message {
            provider_message_id: None,
            role: AgentRuntimeMessageRole::User,
            content: text().unwrap_or_default(),
            final_output: false,
        }),
        "tool_call" | "tool_use" | "function_call" | "tool_start" => {
            Ok(TypedProviderEvent::ToolCall {
                id: value(&["id", "call_id", "tool_call_id"])
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                name: value(&["name", "tool_name", "tool"])
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_tool")
                    .to_string(),
                status: if value(&["approval_required", "requires_approval"])
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    AgentRuntimeToolStatus::WaitingApproval
                } else {
                    AgentRuntimeToolStatus::Running
                },
                arguments: value(&["arguments", "input", "args"]).cloned(),
                result: None,
            })
        }
        "tool_result" | "tool_end" | "function_result" => Ok(TypedProviderEvent::ToolCall {
            id: value(&["id", "call_id", "tool_call_id"])
                .and_then(Value::as_str)
                .map(str::to_owned),
            name: value(&["name", "tool_name", "tool"])
                .and_then(Value::as_str)
                .unwrap_or("unknown_tool")
                .to_string(),
            status: if value(&["error", "failed"])
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                AgentRuntimeToolStatus::Failed
            } else {
                AgentRuntimeToolStatus::Succeeded
            },
            arguments: None,
            result: value(&["result", "output", "content"]).cloned(),
        }),
        "approval_requested" | "permission_request" | "tool_approval" => {
            Ok(TypedProviderEvent::ApprovalRequested {
                id: value(&["approval_id", "id", "request_id"])
                    .and_then(Value::as_str)
                    .unwrap_or("unknown-approval")
                    .to_string(),
                tool_call_id: value(&["tool_call_id", "call_id"])
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                tool_name: value(&["tool_name", "name", "tool"])
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_tool")
                    .to_string(),
            })
        }
        "approval_resolved" | "permission_response" | "tool_approval_response" => {
            Ok(TypedProviderEvent::ApprovalResolved {
                id: value(&["approval_id", "id", "request_id"])
                    .and_then(Value::as_str)
                    .unwrap_or("unknown-approval")
                    .to_string(),
                approved: value(&["approved", "allow"])
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                reason: value(&["reason", "message"])
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        "input_requested" | "input_request" | "question" | "ask_user" => {
            Ok(TypedProviderEvent::InputRequested {
                id: value(&["input_id", "id", "request_id"])
                    .and_then(Value::as_str)
                    .unwrap_or("unknown-input")
                    .to_string(),
                prompt: value(&["prompt", "question", "message"])
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        }
        "input_resolved" | "question_answered" | "answer" => {
            Ok(TypedProviderEvent::InputResolved {
                id: value(&["input_id", "id", "request_id"])
                    .and_then(Value::as_str)
                    .unwrap_or("unknown-input")
                    .to_string(),
                answered: value(&["answered", "ok"])
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
        }
        "usage" | "token_usage" | "tokens" => Ok(TypedProviderEvent::TokenUsage {
            input_tokens: value(&["input_tokens", "prompt_tokens"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
            output_tokens: value(&["output_tokens", "completion_tokens"])
                .and_then(Value::as_u64)
                .unwrap_or(0),
            cached_input_tokens: value(&["cached_input_tokens", "cache_read_tokens"])
                .and_then(Value::as_u64),
        }),
        "result" | "completed" | "done" | "success" => {
            Ok(TypedProviderEvent::Lifecycle(AgentRunStatus::Succeeded))
        }
        "cancelled" | "canceled" | "abort" => {
            Ok(TypedProviderEvent::Lifecycle(AgentRunStatus::Cancelled))
        }
        "error" | "failure" | "failed" | "protocol_error" => Ok(TypedProviderEvent::Error(
            AgentRuntimeError::new(
                if event_type.contains("protocol") {
                    AgentRuntimeErrorKind::ProtocolFailed
                } else {
                    AgentRuntimeErrorKind::Unknown
                },
                value(&["message", "error", "reason"])
                    .and_then(Value::as_str)
                    .unwrap_or("provider error"),
            )
            .with_provider(Some(provider.id())),
        )),
        "eof" | "exit" | "terminal" => Err(NativeAuditError::MalformedFrame(event.sequence)),
        _ => Ok(TypedProviderEvent::Unknown {
            event_type,
            payload: payload.clone(),
        }),
    }
}

/// Decode the event shapes emitted by both Codex app-server and
/// `codex exec --json`. The generic mapper only looks at top-level fields, but
/// Codex puts item identity and content under `params`/`item`.
fn classify_codex_payload(
    payload: &Value,
    sequence: u64,
) -> Option<Result<TypedProviderEvent, NativeAuditError>> {
    let event_type = payload
        .get("method")
        .or_else(|| payload.get("type"))
        .and_then(Value::as_str)?
        .to_ascii_lowercase();

    let params = payload.get("params").unwrap_or(payload);
    match event_type.as_str() {
        "thread/goal/updated" | "thread.goal.updated" => {
            let goal = params.get("goal")?;
            let status = match goal.get("status").and_then(Value::as_str)? {
                "active" => crate::runtime::AgentGoalStatus::Active,
                "paused" => crate::runtime::AgentGoalStatus::Paused,
                "blocked" => crate::runtime::AgentGoalStatus::Blocked,
                "usageLimited" | "usage_limited" => crate::runtime::AgentGoalStatus::UsageLimited,
                "budgetLimited" | "budget_limited" => {
                    crate::runtime::AgentGoalStatus::BudgetLimited
                }
                "complete" => crate::runtime::AgentGoalStatus::Complete,
                _ => return Some(Err(NativeAuditError::MalformedFrame(sequence))),
            };
            let objective = goal.get("objective").and_then(Value::as_str);
            Some(match objective {
                Some(objective) => Ok(TypedProviderEvent::GoalUpdated(
                    crate::runtime::AgentGoalState {
                        objective: objective.to_string(),
                        status,
                        token_budget: goal
                            .get("tokenBudget")
                            .or_else(|| goal.get("token_budget"))
                            .and_then(Value::as_i64),
                        tokens_used: goal
                            .get("tokensUsed")
                            .or_else(|| goal.get("tokens_used"))
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                        time_used_seconds: goal
                            .get("timeUsedSeconds")
                            .or_else(|| goal.get("time_used_seconds"))
                            .and_then(Value::as_i64)
                            .unwrap_or(0),
                    },
                )),
                None => Err(NativeAuditError::MalformedFrame(sequence)),
            })
        }
        "thread/goal/cleared" | "thread.goal.cleared" => Some(Ok(TypedProviderEvent::GoalCleared)),
        "thread/started" | "thread.started" => {
            let session_id = params
                .pointer("/thread/id")
                .or_else(|| params.get("threadId"))
                .or_else(|| params.get("thread_id"))
                .and_then(Value::as_str);
            Some(
                session_id
                    .map(|id| TypedProviderEvent::SessionObserved(id.to_string()))
                    .ok_or(NativeAuditError::MalformedFrame(sequence)),
            )
        }
        "item/agentmessage/delta" | "item.agent_message.delta" => {
            let message_id = params
                .get("itemId")
                .or_else(|| params.get("item_id"))
                .and_then(Value::as_str);
            let delta = params.get("delta").and_then(Value::as_str);
            Some(match (message_id, delta) {
                (Some(message_id), Some(delta)) => Ok(TypedProviderEvent::MessageDelta {
                    provider_message_id: message_id.to_string(),
                    role: AgentRuntimeMessageRole::Assistant,
                    delta: delta.to_string(),
                }),
                _ => Err(NativeAuditError::MalformedFrame(sequence)),
            })
        }
        // Summary deltas remain in the lossless native audit. The completed
        // reasoning item is authoritative, and mapping both would duplicate
        // text because canonical Thinking does not carry a provider item ID.
        "item/reasoning/summarytextdelta" | "item.reasoning.summary_text_delta" => {
            let item_id = params
                .get("itemId")
                .or_else(|| params.get("item_id"))
                .and_then(Value::as_str);
            let delta = params.get("delta").and_then(Value::as_str);
            Some(match (item_id, delta) {
                (Some(item_id), Some(delta)) => Ok(TypedProviderEvent::ThinkingDelta {
                    provider_item_id: item_id.to_string(),
                    delta: delta.to_string(),
                }),
                _ => Err(NativeAuditError::MalformedFrame(sequence)),
            })
        }
        "item/commandexecution/outputdelta" | "item.command_execution.output_delta" => {
            let item_id = params
                .get("itemId")
                .or_else(|| params.get("item_id"))
                .and_then(Value::as_str);
            let delta = params.get("delta").and_then(Value::as_str);
            Some(match (item_id, delta) {
                (Some(item_id), Some(delta)) => Ok(TypedProviderEvent::ToolOutputDelta {
                    provider_item_id: item_id.to_string(),
                    delta: delta.to_string(),
                }),
                _ => Err(NativeAuditError::MalformedFrame(sequence)),
            })
        }
        "thread/tokenusage/updated" | "thread.token_usage.updated" => {
            let usage = params
                .pointer("/tokenUsage/total")
                .or_else(|| params.pointer("/token_usage/total"))?;
            Some(Ok(TypedProviderEvent::TokenUsage {
                input_tokens: codex_u64(usage, &["inputTokens", "input_tokens"]),
                output_tokens: codex_u64(usage, &["outputTokens", "output_tokens"]),
                cached_input_tokens: codex_optional_u64(
                    usage,
                    &["cachedInputTokens", "cached_input_tokens"],
                ),
            }))
        }
        "item/started" | "item.started" => {
            let item = params.get("item")?;
            classify_codex_item(item, false, sequence)
        }
        "item/completed" | "item.completed" => {
            let item = params.get("item")?;
            classify_codex_item(item, true, sequence)
        }
        _ => None,
    }
}

fn classify_codex_item(
    item: &Value,
    completed: bool,
    sequence: u64,
) -> Option<Result<TypedProviderEvent, NativeAuditError>> {
    let item_type = item.get("type").and_then(Value::as_str)?;
    let normalized_type = item_type.replace('_', "").to_ascii_lowercase();

    match normalized_type.as_str() {
        // User input and hook prompts are already represented elsewhere in the
        // canonical conversation and must not be echoed as assistant output.
        "usermessage" | "hookprompt" => Some(Ok(TypedProviderEvent::AuditOnly {
            event_type: format!("item/{item_type}"),
        })),
        "agentmessage" => {
            if !completed {
                return Some(Ok(TypedProviderEvent::AuditOnly {
                    event_type: "item/started/agentMessage".to_string(),
                }));
            }
            let message_id = item.get("id").and_then(Value::as_str);
            let text = item
                .get("text")
                .or_else(|| item.get("message"))
                .or_else(|| item.get("content"))
                .and_then(Value::as_str);
            let final_output = item
                .get("phase")
                .and_then(Value::as_str)
                .is_none_or(|phase| phase != "commentary");
            Some(match (message_id, text) {
                (Some(message_id), Some(text)) => Ok(TypedProviderEvent::Message {
                    provider_message_id: Some(message_id.to_string()),
                    role: AgentRuntimeMessageRole::Assistant,
                    content: text.to_string(),
                    final_output,
                }),
                _ => Err(NativeAuditError::MalformedFrame(sequence)),
            })
        }
        "reasoning" => {
            if !completed {
                return Some(Ok(TypedProviderEvent::AuditOnly {
                    event_type: "item/started/reasoning".to_string(),
                }));
            }
            let summary = item
                .get("summary")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .unwrap_or_default();
            if summary.is_empty() {
                Some(Ok(TypedProviderEvent::AuditOnly {
                    event_type: "item/completed/reasoning-empty".to_string(),
                }))
            } else {
                Some(Ok(TypedProviderEvent::Thinking(summary)))
            }
        }
        "plan" => {
            if !completed {
                return Some(Ok(TypedProviderEvent::AuditOnly {
                    event_type: "item/started/plan".to_string(),
                }));
            }
            Some(
                item.get("text")
                    .and_then(Value::as_str)
                    .map(|text| TypedProviderEvent::Thinking(text.to_string()))
                    .ok_or(NativeAuditError::MalformedFrame(sequence)),
            )
        }
        _ => Some(codex_tool_event(
            item,
            item_type,
            normalized_type.as_str(),
            completed,
            sequence,
        )),
    }
}

fn codex_tool_event(
    item: &Value,
    item_type: &str,
    normalized_type: &str,
    completed: bool,
    sequence: u64,
) -> Result<TypedProviderEvent, NativeAuditError> {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or(NativeAuditError::MalformedFrame(sequence))?;
    let name = match normalized_type {
        "commandexecution" => "Shell".to_string(),
        "filechange" => "File changes".to_string(),
        "mcptoolcall" => {
            let server = item.get("server").and_then(Value::as_str);
            let tool = item.get("tool").and_then(Value::as_str);
            match (server, tool) {
                (Some(server), Some(tool)) => format!("{server}/{tool}"),
                (_, Some(tool)) => tool.to_string(),
                _ => "MCP tool".to_string(),
            }
        }
        "dynamictoolcall" => item
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("Dynamic tool")
            .to_string(),
        "collabagenttoolcall" => "Collaboration".to_string(),
        "subagentactivity" => "Sub-agent activity".to_string(),
        "websearch" => "Web search".to_string(),
        "imageview" => "View image".to_string(),
        "sleep" => "Wait".to_string(),
        "imagegeneration" => "Generate image".to_string(),
        "enteredreviewmode" => "Enter review mode".to_string(),
        "exitedreviewmode" => "Exit review mode".to_string(),
        "contextcompaction" => "Context compaction".to_string(),
        _ => format!("Codex {item_type}"),
    };

    let status = codex_tool_status(item.get("status"), completed);
    let arguments = codex_tool_arguments(item, normalized_type);
    let result = completed.then(|| codex_tool_result(item, normalized_type));

    Ok(TypedProviderEvent::ToolCall {
        id: Some(id.to_string()),
        name,
        status,
        arguments,
        result,
    })
}

fn codex_tool_status(status: Option<&Value>, completed: bool) -> AgentRuntimeToolStatus {
    let status = status
        .and_then(Value::as_str)
        .unwrap_or(if completed { "completed" } else { "running" })
        .replace(['_', '-'], "")
        .to_ascii_lowercase();
    match status.as_str() {
        "completed" | "success" | "succeeded" => AgentRuntimeToolStatus::Succeeded,
        "failed" | "error" => AgentRuntimeToolStatus::Failed,
        "declined" | "denied" | "cancelled" | "canceled" => AgentRuntimeToolStatus::Denied,
        "timedout" => AgentRuntimeToolStatus::TimedOut,
        "waitingapproval" | "pendingapproval" => AgentRuntimeToolStatus::WaitingApproval,
        "approved" => AgentRuntimeToolStatus::Approved,
        "created" | "pending" => AgentRuntimeToolStatus::Created,
        _ => AgentRuntimeToolStatus::Running,
    }
}

fn codex_tool_arguments(item: &Value, normalized_type: &str) -> Option<Value> {
    let fields: &[&str] = match normalized_type {
        "commandexecution" => &["command", "cwd", "source", "commandActions"],
        "filechange" => &["changes"],
        "mcptoolcall" => &["server", "tool", "arguments", "appContext", "pluginId"],
        "dynamictoolcall" => &["namespace", "tool", "arguments"],
        "collabagenttoolcall" => &[
            "tool",
            "senderThreadId",
            "receiverThreadIds",
            "prompt",
            "model",
            "reasoningEffort",
        ],
        "subagentactivity" => &["kind", "agentThreadId", "agentPath"],
        "websearch" => &["query", "action"],
        "imageview" => &["path"],
        "sleep" => &["durationMs"],
        "imagegeneration" => &["prompt", "quality", "size"],
        "enteredreviewmode" | "exitedreviewmode" => &["review"],
        "contextcompaction" => &[],
        _ => return Some(item.clone()),
    };
    Some(codex_object_fields(item, fields))
}

fn codex_tool_result(item: &Value, normalized_type: &str) -> Value {
    let fields: &[&str] = match normalized_type {
        "commandexecution" => &["aggregatedOutput", "exitCode", "durationMs"],
        "filechange" => &["status"],
        "mcptoolcall" => &["result", "error", "durationMs"],
        "dynamictoolcall" => &["contentItems", "success", "durationMs"],
        "collabagenttoolcall" => &["status", "receiverThreadIds", "agentsStates"],
        "subagentactivity" => &["kind"],
        "websearch" => &["results"],
        "imageview" | "sleep" | "enteredreviewmode" | "exitedreviewmode" | "contextcompaction" => {
            &[]
        }
        "imagegeneration" => &["result", "output", "path"],
        _ => return item.clone(),
    };
    codex_object_fields(item, fields)
}

fn codex_object_fields(value: &Value, fields: &[&str]) -> Value {
    let mut selected = serde_json::Map::new();
    for field in fields {
        if let Some(value) = value.get(*field) {
            selected.insert((*field).to_string(), value.clone());
        }
    }
    Value::Object(selected)
}

fn codex_u64(value: &Value, fields: &[&str]) -> u64 {
    codex_optional_u64(value, fields).unwrap_or(0)
}

fn codex_optional_u64(value: &Value, fields: &[&str]) -> Option<u64> {
    fields
        .iter()
        .find_map(|field| value.get(*field))
        .and_then(Value::as_u64)
}

fn map_typed_event_v1(
    provider: DirectProvider,
    event: &DecodedProviderEvent,
    manifest: &NativeAuditManifest,
) -> Result<Vec<AgentEvent>, NativeAuditError> {
    let payload = match &event.typed {
        TypedProviderEvent::MessageDelta {
            provider_message_id,
            role,
            delta,
        } => AgentEventPayload::Message {
            message: crate::runtime::CanonicalMessage {
                message_id: canonical_provider_message_id(
                    manifest.run_attempt_id,
                    provider_message_id,
                ),
                role: *role,
                content: delta.clone(),
            },
            final_output: false,
        },
        TypedProviderEvent::TokenUsage {
            input_tokens,
            output_tokens,
            cached_input_tokens,
        } => AgentEventPayload::TokenUsage {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            cached_input_tokens: *cached_input_tokens,
        },
        TypedProviderEvent::GoalUpdated(goal) => {
            AgentEventPayload::GoalUpdated { goal: goal.clone() }
        }
        TypedProviderEvent::GoalCleared => AgentEventPayload::GoalCleared,
        TypedProviderEvent::ThinkingDelta { .. }
        | TypedProviderEvent::ToolOutputDelta { .. }
        | TypedProviderEvent::AuditOnly { .. }
        | TypedProviderEvent::Unknown { .. } => {
            let raw_payload: Value = serde_json::from_slice(&event.raw.payload)
                .map_err(|_| NativeAuditError::MalformedFrame(event.raw.sequence))?;
            let provider_event = raw_payload
                .get("method")
                .or_else(|| raw_payload.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_ascii_lowercase();
            AgentEventPayload::ProviderExtension {
                provider_namespace: provider.id().to_string(),
                provider_event,
                payload: raw_payload,
            }
        }
        _ => return Ok(project_typed_event(provider, event, manifest)?.durable_events),
    };
    Ok(vec![AgentEventEnvelope {
        schema_version: AGENT_EVENT_SCHEMA_VERSION,
        payload_version: AGENT_EVENT_PAYLOAD_VERSION,
        event_id: event_id(manifest.run_attempt_id, event.raw.sequence),
        session_id: manifest.session_id,
        agent_run_id: manifest.agent_run_id,
        turn_id: manifest.turn_id,
        run_attempt_id: manifest.run_attempt_id,
        run_attempt_number: manifest.run_attempt_number,
        sequence: event.raw.sequence,
        correlation_id: event.raw.correlation_id,
        orchestration_run_id: None,
        orchestration_node_execution_id: None,
        timestamp: event.raw.timestamp,
        native_refs: vec![event.raw.native_ref.clone()],
        payload,
    }])
}

fn project_typed_event(
    provider: DirectProvider,
    event: &DecodedProviderEvent,
    manifest: &NativeAuditManifest,
) -> Result<ProviderEventProjection, NativeAuditError> {
    let live_payload = match &event.typed {
        TypedProviderEvent::MessageDelta {
            provider_message_id,
            role,
            delta,
        } => Some(AgentLiveEventPayload::MessageDelta {
            message_id: canonical_provider_message_id(manifest.run_attempt_id, provider_message_id),
            provider_item_id: provider_message_id.clone(),
            role: *role,
            delta: delta.clone(),
        }),
        TypedProviderEvent::ThinkingDelta {
            provider_item_id,
            delta,
        } => Some(AgentLiveEventPayload::ThinkingDelta {
            provider_item_id: provider_item_id.clone(),
            delta: delta.clone(),
        }),
        TypedProviderEvent::ToolOutputDelta {
            provider_item_id,
            delta,
        } => Some(AgentLiveEventPayload::ToolOutputDelta {
            provider_item_id: provider_item_id.clone(),
            delta: delta.clone(),
        }),
        TypedProviderEvent::TokenUsage {
            input_tokens,
            output_tokens,
            cached_input_tokens,
        } => Some(AgentLiveEventPayload::TokenUsageSnapshot {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            cached_input_tokens: *cached_input_tokens,
        }),
        _ => None,
    };
    if let Some(payload) = live_payload {
        return Ok(ProviderEventProjection {
            durable_events: Vec::new(),
            live_events: vec![AgentLiveEvent {
                schema_version: AGENT_LIVE_EVENT_SCHEMA_VERSION,
                event_id: event_id(manifest.run_attempt_id, event.raw.sequence),
                session_id: manifest.session_id,
                agent_run_id: manifest.agent_run_id,
                turn_id: manifest.turn_id,
                run_attempt_id: manifest.run_attempt_id,
                run_attempt_number: manifest.run_attempt_number,
                native_sequence: event.raw.sequence,
                timestamp: event.raw.timestamp,
                payload,
            }],
        });
    }

    // Known audit-only notifications and unknown optional notifications remain
    // available losslessly in Native Audit, but have no durable consumer.
    if matches!(
        &event.typed,
        TypedProviderEvent::AuditOnly { .. } | TypedProviderEvent::Unknown { .. }
    ) {
        return Ok(ProviderEventProjection {
            durable_events: Vec::new(),
            live_events: Vec::new(),
        });
    }

    let payload = match &event.typed {
        TypedProviderEvent::AgentActivity(activity) => AgentEventPayload::AgentActivity {
            activity: activity.clone(),
        },
        TypedProviderEvent::Lifecycle(status) => {
            AgentEventPayload::LifecycleChanged { status: *status }
        }
        TypedProviderEvent::SessionObserved(session_id) => AgentEventPayload::SessionObserved {
            provider_session: ProviderSessionReference {
                schema_version: crate::runtime::PROVIDER_SESSION_REFERENCE_SCHEMA_VERSION,
                provider_id: provider.id().to_string(),
                runtime_profile_id: manifest.runtime_profile_id.clone(),
                provider_session_id: session_id.clone(),
                observed_at: event.raw.timestamp,
                metadata: Some(serde_json::json!({ "source": "native_frame" })),
            },
        },
        TypedProviderEvent::Message {
            provider_message_id,
            role,
            content,
            final_output,
        } => AgentEventPayload::Message {
            message: crate::runtime::CanonicalMessage {
                message_id: provider_message_id
                    .as_deref()
                    .map(|message_id| {
                        canonical_provider_message_id(manifest.run_attempt_id, message_id)
                    })
                    .unwrap_or_else(|| event_id(manifest.run_attempt_id, event.raw.sequence)),
                role: *role,
                content: content.clone(),
            },
            final_output: *final_output,
        },
        TypedProviderEvent::Thinking(content) => AgentEventPayload::Thinking {
            content: content.clone(),
        },
        TypedProviderEvent::ToolCall {
            id,
            name,
            status,
            arguments,
            result,
        } => AgentEventPayload::ToolCall {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            status: *status,
            arguments: arguments.clone(),
            result: result.clone(),
        },
        TypedProviderEvent::ApprovalRequested {
            id,
            tool_call_id,
            tool_name,
        } => AgentEventPayload::ApprovalRequested {
            approval_id: id.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
        },
        TypedProviderEvent::ApprovalResolved {
            id,
            approved,
            reason,
        } => AgentEventPayload::ApprovalResolved {
            approval_id: id.clone(),
            approved: *approved,
            reason: reason.clone(),
        },
        TypedProviderEvent::InputRequested { id, prompt } => AgentEventPayload::InputRequested {
            input_id: id.clone(),
            prompt: prompt.clone(),
        },
        TypedProviderEvent::InputResolved { id, answered } => AgentEventPayload::InputResolved {
            input_id: id.clone(),
            answered: *answered,
        },
        TypedProviderEvent::Error(error) => AgentEventPayload::Error {
            error: error.clone(),
        },
        TypedProviderEvent::GoalUpdated(goal) => {
            AgentEventPayload::GoalUpdated { goal: goal.clone() }
        }
        TypedProviderEvent::GoalCleared => AgentEventPayload::GoalCleared,
        TypedProviderEvent::MessageDelta { .. }
        | TypedProviderEvent::ThinkingDelta { .. }
        | TypedProviderEvent::ToolOutputDelta { .. }
        | TypedProviderEvent::TokenUsage { .. }
        | TypedProviderEvent::AuditOnly { .. }
        | TypedProviderEvent::Unknown { .. } => unreachable!("handled above"),
    };
    Ok(ProviderEventProjection {
        durable_events: vec![AgentEventEnvelope {
            schema_version: AGENT_EVENT_SCHEMA_VERSION,
            payload_version: AGENT_EVENT_PAYLOAD_VERSION,
            event_id: event_id(manifest.run_attempt_id, event.raw.sequence),
            session_id: manifest.session_id,
            agent_run_id: manifest.agent_run_id,
            turn_id: manifest.turn_id,
            run_attempt_id: manifest.run_attempt_id,
            run_attempt_number: manifest.run_attempt_number,
            sequence: event.raw.sequence,
            correlation_id: event.raw.correlation_id,
            orchestration_run_id: None,
            orchestration_node_execution_id: None,
            timestamp: event.raw.timestamp,
            native_refs: vec![event.raw.native_ref.clone()],
            payload,
        }],
        live_events: Vec::new(),
    })
}

fn event_id(run_attempt_id: Uuid, sequence: u64) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(run_attempt_id.as_bytes());
    hasher.update(sequence.to_be_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hasher.finalize()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn canonical_provider_message_id(run_attempt_id: Uuid, provider_message_id: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(run_attempt_id.as_bytes());
    hasher.update(b"provider-message:");
    hasher.update(provider_message_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hasher.finalize()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Serialize one NDJSON request without appending a second frame or accepting
/// an embedded SDK.  Callers write the returned bytes directly to `omp` stdin.
pub fn encode_stdio_rpc(request: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(request)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Provider-neutral repository memory guidance. Codex receives this through
/// persistent developer instructions; other providers receive a self-contained
/// prompt suffix. Do not change slash-command parsing or Codex Goal objectives.
pub fn prompt_with_repository_memory(
    provider: DirectProvider,
    prompt: &str,
    env: &crate::env::ExecutionEnv,
) -> String {
    if provider != DirectProvider::Codex
        && !prompt.trim_start().starts_with('/')
        && let Some(instructions) = env.get("EVK_REPOSITORY_MEMORY_INSTRUCTIONS")
    {
        return format!(
            "{prompt}\n\n{}\n\n## EVK repository memory context\n{instructions}",
            crate::legacy_wiki::RETIREMENT_NOTICE
        );
    }
    if provider != DirectProvider::Codex && !prompt.trim_start().starts_with('/') {
        return format!("{prompt}\n\n{}", crate::legacy_wiki::RETIREMENT_NOTICE);
    }
    prompt.to_owned()
}

/// Control-only calls must never sweep pre-existing dirty source into a commit.
/// Keep provider slash parsing here, not in the repository-memory service.
pub fn memory_source_completion_allowed(
    provider: DirectProvider,
    intent: DirectIntent,
    prompt: &str,
) -> bool {
    if intent == DirectIntent::Review {
        return false;
    }
    if provider == DirectProvider::Codex {
        use super::codex::slash_commands::{CodexGoalCommand, CodexSlashCommand};
        return matches!(
            CodexSlashCommand::parse(prompt),
            None | Some(CodexSlashCommand::Init)
                | Some(CodexSlashCommand::Goal(
                    CodexGoalCommand::Set { .. } | CodexGoalCommand::Resume
                ))
        );
    }
    !prompt.trim_start().starts_with('/')
}

#[cfg(test)]
mod tests {
    #[test]
    fn repository_memory_does_not_commit_on_compaction_review_or_control_commands() {
        for prompt in [
            "/compact",
            "/compact keep design",
            "/status",
            "/goal",
            "/goal pause",
            "/review",
        ] {
            assert!(
                !memory_source_completion_allowed(
                    DirectProvider::Codex,
                    DirectIntent::FollowUp,
                    prompt
                ),
                "{prompt}"
            );
        }
        for prompt in [
            "implement feature",
            "/goal implement feature",
            "/goal resume",
            "/init",
        ] {
            assert!(
                memory_source_completion_allowed(
                    DirectProvider::Codex,
                    DirectIntent::FollowUp,
                    prompt
                ),
                "{prompt}"
            );
        }
        assert!(!memory_source_completion_allowed(
            DirectProvider::Codex,
            DirectIntent::Review,
            "review code"
        ));
    }

    #[test]
    fn codex_native_and_launch_errors_preserve_actionable_reason() {
        let provider = DirectProvider::Codex;
        for payload in [
            serde_json::json!({"id":4,"error":{"code":-32600,"message":"goal objective must be at most 4000 characters"}}),
            serde_json::json!({"LaunchError":{"error":"goal objective must be at most 4000 characters\n\nCodex launch context:\nprivate paths"}}),
        ] {
            let event = provider
                .decode_native_frame(&frame(provider, payload))
                .unwrap();
            let projected = provider
                .map_provider_event(&event, &fixture_manifest(provider))
                .unwrap();
            assert!(
                matches!(&projected[0].payload, crate::runtime::AgentEventPayload::Error { error } if error.message == "goal objective must be at most 4000 characters")
            );
        }
    }
    use chrono::DateTime;
    use serde_json::json;

    use super::*;
    use crate::{
        approvals::NoopExecutorApprovalService,
        env::RepoContext,
        executors::BaseCodingAgent,
        runtime::{NativeAuditChannel, PROVIDER_SESSION_REFERENCE_SCHEMA_VERSION},
    };

    fn frame(provider: DirectProvider, payload: Value) -> NativeAuditFrame {
        frame_at(provider, 1, payload)
    }

    fn frame_at(provider: DirectProvider, sequence: u64, payload: Value) -> NativeAuditFrame {
        NativeAuditFrame::from_bytes(
            sequence,
            DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            crate::runtime::NativeAuditDirection::Output,
            NativeAuditChannel::Stdout,
            "application/json",
            Uuid::from_u128(10),
            serde_json::to_string(&payload).unwrap().as_bytes(),
        )
        .with_metadata(Some(serde_json::json!({ "provider": provider.id() })))
    }

    fn launch_env() -> ExecutionEnv {
        ExecutionEnv::new(
            RepoContext::new(".".into(), Vec::new()),
            false,
            String::new(),
        )
    }

    fn provider_session(
        provider: DirectProvider,
        config: &ExecutorConfig,
    ) -> ProviderSessionReference {
        ProviderSessionReference {
            schema_version: PROVIDER_SESSION_REFERENCE_SCHEMA_VERSION,
            provider_id: provider.id().to_string(),
            runtime_profile_id: config.profile_id().cache_key(),
            provider_session_id: "native-session".to_string(),
            observed_at: Utc::now(),
            metadata: None,
        }
    }

    fn validate_launch(
        provider: DirectProvider,
        config: &ExecutorConfig,
        intent: DirectIntent,
        session: Option<&ProviderSessionReference>,
        reset_to_message_id: Option<&str>,
    ) -> Result<(), ExecutorError> {
        let env = launch_env();
        validate_direct_launch(&DirectProviderLaunchRequest {
            provider,
            executor_config: config,
            intent,
            prompt: "prompt",
            provider_session: session,
            reset_to_message_id,
            selected_skills: &[],
            approvals: Arc::new(NoopExecutorApprovalService),
            current_dir: Path::new("."),
            env: &env,
        })
    }

    #[test]
    fn all_four_versions_use_local_executables() {
        let versions = DirectProvider::ALL.map(DirectProvider::versions);

        assert_eq!(
            versions.map(|version| version.executable),
            ["gemini", "codex", "claude", "omp"]
        );
        assert!(versions.iter().all(|version| version.runtime.is_none()));
        assert_eq!(versions[1].protocol, Some("rust-v0.144.1"));
        assert!(
            versions
                .windows(2)
                .all(|pair| pair[0].adapter != pair[1].adapter)
        );
    }

    #[test]
    fn all_four_profile_identities_map_to_their_direct_provider() {
        let mappings = [
            (BaseCodingAgent::Gemini, DirectProvider::Gemini),
            (BaseCodingAgent::Codex, DirectProvider::Codex),
            (BaseCodingAgent::ClaudeCode, DirectProvider::ClaudeCode),
            (BaseCodingAgent::OhMyPi, DirectProvider::OhMyPi),
        ];

        for (agent, provider) in mappings {
            assert_eq!(DirectProvider::from_base_agent(agent), Some(provider));
            let config = ExecutorConfig::new(agent);
            assert!(validate_launch(provider, &config, DirectIntent::Initial, None, None).is_ok());
        }
    }

    #[test]
    fn profile_provider_mismatch_is_rejected_before_spawn() {
        let error = validate_launch(
            DirectProvider::Gemini,
            &ExecutorConfig::new(BaseCodingAgent::Codex),
            DirectIntent::Initial,
            None,
            None,
        )
        .expect_err("mismatched profile must not launch");

        assert!(matches!(error, ExecutorError::UnknownExecutorType(_)));
    }

    #[test]
    fn follow_up_requires_a_matching_non_empty_provider_session() {
        let config = ExecutorConfig::new(BaseCodingAgent::Codex);
        let error = validate_launch(
            DirectProvider::Codex,
            &config,
            DirectIntent::FollowUp,
            None,
            None,
        )
        .expect_err("follow-up without a session must not launch");
        assert!(matches!(error, ExecutorError::FollowUpNotSupported(_)));

        let mut session = provider_session(DirectProvider::Codex, &config);
        session.runtime_profile_id = "CODEX:OTHER".to_string();
        let error = validate_launch(
            DirectProvider::Codex,
            &config,
            DirectIntent::FollowUp,
            Some(&session),
            None,
        )
        .expect_err("session from another profile must not launch");
        assert!(matches!(error, ExecutorError::FollowUpNotSupported(_)));
    }

    #[test]
    fn native_session_ids_reject_ambiguous_whitespace_and_option_values() {
        assert!(validate_native_session_id(DirectProvider::Gemini, " session ").is_err());
        assert!(validate_native_session_id(DirectProvider::OhMyPi, "-resume").is_err());
        assert!(validate_native_session_id(DirectProvider::Codex, "thread-1").is_ok());
    }

    #[test]
    fn initial_launch_rejects_an_attached_provider_session() {
        let config = ExecutorConfig::new(BaseCodingAgent::Gemini);
        let session = provider_session(DirectProvider::Gemini, &config);

        let error = validate_launch(
            DirectProvider::Gemini,
            &config,
            DirectIntent::Initial,
            Some(&session),
            None,
        )
        .expect_err("initial launch must not accidentally resume");

        assert!(matches!(error, ExecutorError::FollowUpNotSupported(_)));
    }

    #[test]
    fn reset_is_only_allowed_for_claude_follow_up_or_resume() {
        for provider in [
            DirectProvider::Gemini,
            DirectProvider::Codex,
            DirectProvider::OhMyPi,
        ] {
            let agent = match provider {
                DirectProvider::Gemini => BaseCodingAgent::Gemini,
                DirectProvider::Codex => BaseCodingAgent::Codex,
                DirectProvider::OhMyPi => BaseCodingAgent::OhMyPi,
                DirectProvider::ClaudeCode => unreachable!(),
            };
            let config = ExecutorConfig::new(agent);
            let session = provider_session(provider, &config);
            let error = validate_launch(
                provider,
                &config,
                DirectIntent::FollowUp,
                Some(&session),
                Some("message-id"),
            )
            .expect_err("unsupported reset must not silently downgrade");
            assert!(matches!(
                error,
                ExecutorError::ResetToMessageNotSupported(_)
            ));
        }

        let config = ExecutorConfig::new(BaseCodingAgent::ClaudeCode);
        let session = provider_session(DirectProvider::ClaudeCode, &config);
        assert!(
            validate_launch(
                DirectProvider::ClaudeCode,
                &config,
                DirectIntent::Resume,
                Some(&session),
                Some("message-id"),
            )
            .is_ok()
        );
    }

    #[test]
    fn unknown_and_terminal_frames_fail_closed() {
        let unknown = DirectProvider::Gemini
            .decode_native_frame(&frame(
                DirectProvider::Gemini,
                serde_json::json!({"type":"future_event"}),
            ))
            .unwrap();
        assert!(matches!(unknown.typed, TypedProviderEvent::Unknown { .. }));
        assert_eq!(unknown.typed.class(), ProviderEventClass::AuditOnly);
        assert!(
            DirectProvider::Gemini
                .decode_native_frame(&frame(
                    DirectProvider::Gemini,
                    serde_json::json!({"type":"eof"})
                ))
                .is_err()
        );
        assert!(matches!(
            DirectProvider::Codex.classify_native_frame(&frame(
                DirectProvider::Codex,
                serde_json::json!({
                    "method": "item/agentMessage/delta",
                    "params": {"itemId": "message-without-delta"}
                }),
            )),
            ProviderFrameClassification::UnsupportedRequired { .. }
        ));
    }

    #[test]
    fn maps_codex_app_server_stream_to_one_final_assistant_message() {
        let provider = DirectProvider::Codex;
        let manifest = fixture_manifest(provider);
        let delta = provider
            .decode_native_frame(&frame_at(
                provider,
                1,
                serde_json::json!({
                    "method": "item/agentMessage/delta",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "itemId": "message-1",
                        "delta": "Done"
                    },
                    "emittedAtMs": 1_787_827_905_271_i64
                }),
            ))
            .unwrap();
        let completed = provider
            .decode_native_frame(&frame_at(
                provider,
                2,
                serde_json::json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": "thread-1",
                        "turnId": "turn-1",
                        "item": {
                            "type": "agentMessage",
                            "id": "message-1",
                            "text": "Done",
                            "phase": "final_answer",
                            "delivery": null
                        }
                    },
                    "emittedAtMs": 1_787_827_905_271_i64
                }),
            ))
            .unwrap();

        let delta = provider.project_provider_event(&delta, &manifest).unwrap();
        let completed = provider
            .project_provider_event(&completed, &manifest)
            .unwrap();
        assert!(delta.durable_events.is_empty());
        let AgentLiveEventPayload::MessageDelta {
            message_id: delta_message_id,
            delta: delta_content,
            ..
        } = &delta.live_events[0].payload
        else {
            panic!("Codex agent message delta must be live-only");
        };
        let AgentEventPayload::Message {
            message: completed_message,
            final_output: completed_final,
        } = &completed.durable_events[0].payload
        else {
            panic!("completed Codex agent message must be durable");
        };

        assert_eq!(*delta_message_id, completed_message.message_id);
        assert_eq!(delta_content, "Done");
        assert_eq!(completed_message.content, "Done");
        assert!(*completed_final);
    }

    #[test]
    fn thousand_codex_deltas_produce_live_updates_but_no_canonical_events() {
        let provider = DirectProvider::Codex;
        let manifest = fixture_manifest(provider);
        let mut durable_count = 0;
        let mut live_count = 0;
        for sequence in 1..=1_000 {
            let decoded = provider
                .decode_native_frame(&frame_at(
                    provider,
                    sequence,
                    serde_json::json!({
                        "method": "item/agentMessage/delta",
                        "params": {"itemId": "message-1", "delta": "x"}
                    }),
                ))
                .unwrap();
            let projection = provider
                .project_provider_event(&decoded, &manifest)
                .unwrap();
            durable_count += projection.durable_events.len();
            live_count += projection.live_events.len();
        }
        assert_eq!(durable_count, 0);
        assert_eq!(live_count, 1_000);
    }

    #[test]
    fn codex_reasoning_and_command_output_deltas_are_live_only() {
        let provider = DirectProvider::Codex;
        let manifest = fixture_manifest(provider);
        for (payload, expected_item_id) in [
            (
                serde_json::json!({
                    "method": "item/reasoning/summaryTextDelta",
                    "params": {"itemId": "reasoning-1", "delta": "summary"}
                }),
                "reasoning-1",
            ),
            (
                serde_json::json!({
                    "method": "item/commandExecution/outputDelta",
                    "params": {"itemId": "command-1", "delta": "stdout"}
                }),
                "command-1",
            ),
        ] {
            let decoded = provider
                .decode_native_frame(&frame(provider, payload))
                .unwrap();
            assert_eq!(decoded.typed.class(), ProviderEventClass::LiveOnly);
            let projection = provider
                .project_provider_event(&decoded, &manifest)
                .unwrap();
            assert!(projection.durable_events.is_empty());
            assert!(matches!(
                projection.live_events.as_slice(),
                [AgentLiveEvent {
                    payload: AgentLiveEventPayload::ThinkingDelta { provider_item_id, .. }
                        | AgentLiveEventPayload::ToolOutputDelta { provider_item_id, .. },
                    ..
                }] if provider_item_id == expected_item_id
            ));
        }
    }

    #[test]
    fn native_audit_keeps_thousand_deltas_while_v2_replay_compacts_history() {
        let provider = DirectProvider::Codex;
        let root = tempfile::tempdir().unwrap();
        let ids = [
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        ];
        let versions = provider.versions();
        let mut writer = crate::runtime::NativeAuditWriter::create_in(
            root.path(),
            crate::runtime::NativeAuditMetadata {
                session_id: ids[0],
                agent_run_id: ids[1],
                turn_id: ids[2],
                run_attempt_id: ids[3],
                run_attempt_number: 1,
                provider_id: provider.id().to_string(),
                runtime_profile_id: "default".to_string(),
                workspace_path: root.path().display().to_string(),
                runtime_version: versions.runtime.map(str::to_owned),
                protocol_version: versions.protocol.map(str::to_owned),
                adapter_version: versions.adapter.to_string(),
                mapper_version: provider.semantic_mapper().versions().mapper_version,
                created_at: Utc::now(),
            },
        )
        .unwrap();
        for _ in 0..1_000 {
            let payload = serde_json::json!({
                "method": "item/agentMessage/delta",
                "params": {"itemId": "message-1", "delta": "x"}
            });
            writer
                .append_native_output(
                    NativeAuditChannel::Stdout,
                    "application/json",
                    ids[0],
                    serde_json::to_string(&payload).unwrap().as_bytes(),
                )
                .unwrap();
        }
        let completed = serde_json::json!({
            "method": "item/completed",
            "params": {
                "item": {
                    "type": "agentMessage",
                    "id": "message-1",
                    "text": "complete"
                }
            }
        });
        writer
            .append_native_output(
                NativeAuditChannel::Stdout,
                "application/json",
                ids[0],
                serde_json::to_string(&completed).unwrap().as_bytes(),
            )
            .unwrap();
        let manifest = writer.close().unwrap();
        assert_eq!(manifest.last_sequence, Some(1_001));
        assert!(manifest.final_checksum.is_some());

        let directory = root
            .path()
            .join(&manifest.manifest_relative_path)
            .parent()
            .unwrap()
            .to_path_buf();
        let bundle = crate::runtime::AuditBundle::read(&directory).unwrap();
        let replay = bundle.replay(&provider.semantic_mapper()).unwrap();
        assert_eq!(replay.provider_events.len(), 1_001);
        assert_eq!(replay.agent_events.len(), 1);
        assert!(matches!(
            &replay.agent_events[0].payload,
            AgentEventPayload::Message {
                message,
                final_output: true,
            } if message.content == "complete"
        ));
    }

    #[test]
    fn intentional_and_unknown_notifications_do_not_fall_back_to_extensions() {
        let provider = DirectProvider::Codex;
        let manifest = fixture_manifest(provider);
        for payload in [
            serde_json::json!({
                "method": "item/started",
                "params": {"item": {"type": "agentMessage", "id": "message-1"}}
            }),
            serde_json::json!({"method": "future/optionalNotification", "params": {}}),
        ] {
            let decoded = provider
                .decode_native_frame(&frame(provider, payload))
                .unwrap();
            let projection = provider
                .project_provider_event(&decoded, &manifest)
                .unwrap();
            assert!(projection.durable_events.is_empty());
            assert!(projection.live_events.is_empty());
        }
    }

    #[test]
    fn maps_codex_published_reasoning_summary_without_private_content() {
        let event = DirectProvider::Codex
            .decode_native_frame(&frame(
                DirectProvider::Codex,
                serde_json::json!({
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "type": "reasoning",
                            "id": "reasoning-1",
                            "summary": ["Inspecting the project", "Checking the tests"],
                            "content": ["provider-private reasoning must not be projected"]
                        }
                    }
                }),
            ))
            .unwrap();

        assert!(matches!(
            event.typed,
            TypedProviderEvent::Thinking(ref content)
                if content == "Inspecting the project\n\nChecking the tests"
                    && !content.contains("provider-private")
        ));
    }

    #[test]
    fn maps_codex_command_lifecycle_with_output() {
        let provider = DirectProvider::Codex;
        let started = provider
            .decode_native_frame(&frame_at(
                provider,
                1,
                serde_json::json!({
                    "method": "item/started",
                    "params": {
                        "item": {
                            "type": "commandExecution",
                            "id": "command-1",
                            "command": "pwd",
                            "cwd": "/workspace",
                            "status": "inProgress",
                            "commandActions": []
                        }
                    }
                }),
            ))
            .unwrap();
        let completed = provider
            .decode_native_frame(&frame_at(
                provider,
                2,
                serde_json::json!({
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "type": "commandExecution",
                            "id": "command-1",
                            "command": "pwd",
                            "cwd": "/workspace",
                            "status": "completed",
                            "commandActions": [],
                            "aggregatedOutput": "/workspace\n",
                            "exitCode": 0,
                            "durationMs": 12
                        }
                    }
                }),
            ))
            .unwrap();

        assert!(matches!(
            started.typed,
            TypedProviderEvent::ToolCall {
                id: Some(ref id),
                ref name,
                status: AgentRuntimeToolStatus::Running,
                ref arguments,
                result: None,
            } if id == "command-1"
                && name == "Shell"
                && arguments.as_ref().and_then(|value| value.get("command")) == Some(&json!("pwd"))
        ));
        assert!(matches!(
            completed.typed,
            TypedProviderEvent::ToolCall {
                id: Some(ref id),
                status: AgentRuntimeToolStatus::Succeeded,
                ref result,
                ..
            } if id == "command-1"
                && result.as_ref().and_then(|value| value.get("aggregatedOutput"))
                    == Some(&json!("/workspace\n"))
                && result.as_ref().and_then(|value| value.get("exitCode")) == Some(&json!(0))
        ));
    }

    #[test]
    fn maps_codex_web_search_results_and_total_token_usage() {
        let provider = DirectProvider::Codex;
        let search = provider
            .decode_native_frame(&frame_at(
                provider,
                1,
                serde_json::json!({
                    "method": "item/completed",
                    "params": {
                        "item": {
                            "type": "webSearch",
                            "id": "search-1",
                            "query": "Codex app-server",
                            "action": {"type": "search"},
                            "results": [{"title": "Docs", "url": "https://example.com"}]
                        }
                    }
                }),
            ))
            .unwrap();
        let usage = provider
            .decode_native_frame(&frame_at(
                provider,
                2,
                serde_json::json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "tokenUsage": {
                            "total": {
                                "totalTokens": 42,
                                "inputTokens": 30,
                                "cachedInputTokens": 10,
                                "outputTokens": 12,
                                "reasoningOutputTokens": 4
                            },
                            "last": {
                                "totalTokens": 42,
                                "inputTokens": 30,
                                "cachedInputTokens": 10,
                                "outputTokens": 12,
                                "reasoningOutputTokens": 4
                            }
                        }
                    }
                }),
            ))
            .unwrap();

        assert!(matches!(
            search.typed,
            TypedProviderEvent::ToolCall {
                id: Some(ref id),
                ref name,
                status: AgentRuntimeToolStatus::Succeeded,
                ref arguments,
                ref result,
            } if id == "search-1"
                && name == "Web search"
                && arguments.as_ref().and_then(|value| value.get("query"))
                    == Some(&json!("Codex app-server"))
                && result.as_ref().and_then(|value| value.get("results"))
                    .and_then(Value::as_array).is_some_and(|results| results.len() == 1)
        ));
        assert!(matches!(
            usage.typed,
            TypedProviderEvent::TokenUsage {
                input_tokens: 30,
                output_tokens: 12,
                cached_input_tokens: Some(10),
            }
        ));
        assert_eq!(usage.typed.class(), ProviderEventClass::SnapshotOnly);
        let projected = provider
            .project_provider_event(&usage, &fixture_manifest(provider))
            .unwrap();
        assert!(projected.durable_events.is_empty());
        assert!(matches!(
            projected.live_events.as_slice(),
            [AgentLiveEvent {
                payload: AgentLiveEventPayload::TokenUsageSnapshot {
                    input_tokens: 30,
                    output_tokens: 12,
                    cached_input_tokens: Some(10),
                },
                ..
            }]
        ));
    }

    #[test]
    fn maps_official_codex_jsonl_final_message_and_thread_id() {
        let provider = DirectProvider::Codex;
        let thread = provider
            .decode_native_frame(&frame_at(
                provider,
                1,
                serde_json::json!({
                    "type": "thread.started",
                    "thread_id": "thread-jsonl"
                }),
            ))
            .unwrap();
        assert!(matches!(
            thread.typed,
            TypedProviderEvent::SessionObserved(ref id) if id == "thread-jsonl"
        ));

        let message = provider
            .decode_native_frame(&frame_at(
                provider,
                2,
                serde_json::json!({
                    "type": "item.completed",
                    "item": {
                        "id": "item-3",
                        "type": "agent_message",
                        "text": "Repository summarized."
                    }
                }),
            ))
            .unwrap();
        assert!(matches!(
            message.typed,
            TypedProviderEvent::Message {
                provider_message_id: Some(ref id),
                role: AgentRuntimeMessageRole::Assistant,
                ref content,
                final_output: true,
            } if id == "item-3" && content == "Repository summarized."
        ));
    }

    #[test]
    fn maps_codex_goal_progress_to_canonical_state() {
        let provider = DirectProvider::Codex;
        let manifest = fixture_manifest(provider);
        let decoded = provider
            .decode_native_frame(&frame_at(
                provider,
                1,
                serde_json::json!({
                    "method": "thread/goal/updated",
                    "params": {
                        "threadId": "thread-1",
                        "goal": {
                            "objective": "Ship the feature",
                            "status": "active",
                            "tokenBudget": 50000,
                            "tokensUsed": 1200,
                            "timeUsedSeconds": 42
                        }
                    }
                }),
            ))
            .unwrap();
        let projected = project_typed_event(provider, &decoded, &manifest).unwrap();

        assert!(matches!(
            projected.durable_events.as_slice(),
            [AgentEvent {
                payload: AgentEventPayload::GoalUpdated { goal },
                ..
            }] if goal.objective == "Ship the feature"
                && goal.status == crate::runtime::AgentGoalStatus::Active
                && goal.token_budget == Some(50_000)
                && goal.tokens_used == 1_200
                && goal.time_used_seconds == 42
        ));
        assert!(projected.live_events.is_empty());
    }

    #[test]
    fn maps_approval_and_waiting_input_to_canonical_events() {
        let manifest = NativeAuditManifest {
            audit_schema_version: crate::runtime::NATIVE_AUDIT_SCHEMA_VERSION,
            session_id: Uuid::from_u128(1),
            agent_run_id: Uuid::from_u128(2),
            turn_id: Uuid::from_u128(3),
            run_attempt_id: Uuid::from_u128(4),
            run_attempt_number: 1,
            provider_id: "gemini".to_string(),
            runtime_profile_id: "default".to_string(),
            workspace_path: ".".to_string(),
            runtime_version: None,
            protocol_version: Some("acp-0.8".to_string()),
            adapter_version: "gemini-adapter-v1".to_string(),
            mapper_version: "gemini-mapper-v1".to_string(),
            frame_count: 1,
            first_sequence: Some(1),
            last_sequence: Some(1),
            final_checksum: None,
            integrity_status: crate::runtime::NativeAuditIntegrityStatus::Complete,
            created_at: Utc::now(),
            closed_at: None,
            manifest_relative_path: "manifest.json".to_string(),
            frames_relative_path: "frames.jsonl".to_string(),
            raw_content_trusted: true,
        };
        let event = DirectProvider::Gemini
            .decode_native_frame(&frame(
                DirectProvider::Gemini,
                serde_json::json!({"type":"approval_requested","approval_id":"a1","tool_name":"shell"}),
            ))
            .unwrap();
        let mapped = DirectProvider::Gemini
            .map_provider_event(&event, &manifest)
            .unwrap();
        assert!(matches!(
            mapped[0].payload,
            AgentEventPayload::ApprovalRequested { .. }
        ));
    }

    #[test]
    fn omp_rpc_is_ndjson_and_external() {
        let request = super::super::oh_my_pi::command_adapter::session_request("hello", None);
        let bytes = encode_stdio_rpc(&request).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("\"type\":\"prompt\"")
        );
        assert_eq!(DirectProvider::OhMyPi.versions().executable, "omp");
    }

    #[test]
    fn controls_are_provider_native_and_fail_closed_on_unknown_capability() {
        let snapshot = DirectProvider::Gemini.capabilities("default");
        assert_eq!(
            snapshot.resolve(crate::runtime::AgentCapability::Approval),
            CapabilityState::Native
        );
        let error = encode_control(
            DirectProvider::Gemini,
            &snapshot,
            DirectControl::Approve {
                request_id: "a1".to_string(),
                approved: true,
                reason: None,
            },
        )
        .expect_err("ACP approvals must be routed through the active connection");
        assert!(matches!(error, DirectControlError::Json(_)));

        let mut unknown = DirectProvider::Gemini.capabilities("default");
        unknown
            .capabilities
            .retain(|entry| entry.capability != crate::runtime::AgentCapability::Steering);
        let error = encode_control(
            DirectProvider::Gemini,
            &unknown,
            DirectControl::Steer {
                text: "stop".to_string(),
            },
        )
        .expect_err("missing steering capability must not silently fall back");
        assert!(matches!(
            error,
            DirectControlError::Capability(CapabilityGateError::Unresolved { .. })
        ));

        let omp_snapshot = DirectProvider::OhMyPi.capabilities("default");
        assert_eq!(
            omp_snapshot.resolve(crate::runtime::AgentCapability::Approval),
            CapabilityState::Unsupported
        );
        let error = encode_control(
            DirectProvider::OhMyPi,
            &omp_snapshot,
            DirectControl::Approve {
                request_id: "a1".to_string(),
                approved: true,
                reason: None,
            },
        )
        .expect_err("Oh My Pi approval is not exposed by its RPC adapter");
        assert!(matches!(
            error,
            DirectControlError::Capability(CapabilityGateError::Unsupported { .. })
        ));
    }

    #[test]
    fn all_provider_replays_use_the_same_decoder_mapper_reducer_path() {
        for provider in DirectProvider::ALL {
            let root = tempfile::tempdir().unwrap();
            let ids = [
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
            ];
            let versions = provider.versions();
            let mut writer = crate::runtime::NativeAuditWriter::create_in(
                root.path(),
                crate::runtime::NativeAuditMetadata {
                    session_id: ids[0],
                    agent_run_id: ids[1],
                    turn_id: ids[2],
                    run_attempt_id: ids[3],
                    run_attempt_number: 1,
                    provider_id: provider.id().to_string(),
                    runtime_profile_id: "default".to_string(),
                    workspace_path: root.path().display().to_string(),
                    runtime_version: versions.runtime.map(str::to_owned),
                    protocol_version: versions.protocol.map(str::to_owned),
                    adapter_version: versions.adapter.to_string(),
                    mapper_version: versions.mapper.to_string(),
                    created_at: Utc::now(),
                },
            )
            .unwrap();
            for payload in [
                serde_json::json!({"type":"session_started","session_id":"native-1"}),
                serde_json::json!({"type":"message","content":"done","final":true}),
                serde_json::json!({"type":"result"}),
            ] {
                writer
                    .append_native_output(
                        NativeAuditChannel::Stdout,
                        "application/json",
                        ids[0],
                        serde_json::to_string(&payload).unwrap().as_bytes(),
                    )
                    .unwrap();
            }
            let manifest = writer.close().unwrap();
            let directory = root
                .path()
                .join(&manifest.manifest_relative_path)
                .parent()
                .unwrap()
                .to_path_buf();
            let bundle = crate::runtime::AuditBundle::read(&directory).unwrap();
            let first = bundle.replay(&provider.mapper()).unwrap();
            let second = bundle.replay(&provider.mapper()).unwrap();
            assert_eq!(first.provider_events, second.provider_events);
            assert_eq!(first.agent_events, second.agent_events);
            assert_eq!(first.state, second.state);
            assert_eq!(first.state.status, AgentRunStatus::Succeeded);
            assert_eq!(first.state.last_event_sequence, 3);
        }
    }

    #[test]
    fn scoped_audit_replay_preserves_raw_frames_and_parent_answer() {
        let provider = DirectProvider::Codex;
        let root = tempfile::tempdir().unwrap();
        let versions = provider.versions();
        let mut writer = crate::runtime::NativeAuditWriter::create_in(
            root.path(),
            crate::runtime::NativeAuditMetadata {
                session_id: Uuid::new_v4(),
                agent_run_id: Uuid::new_v4(),
                turn_id: Uuid::new_v4(),
                run_attempt_id: Uuid::new_v4(),
                run_attempt_number: 1,
                provider_id: provider.id().into(),
                runtime_profile_id: "default".into(),
                workspace_path: root.path().display().to_string(),
                runtime_version: None,
                protocol_version: versions.protocol.map(str::to_owned),
                adapter_version: versions.adapter.into(),
                mapper_version: versions.mapper.into(),
                created_at: Utc::now(),
            },
        )
        .unwrap();
        let mut frames = vec![
            json!({"result":{"thread":{"id":"root"}}}),
            json!({"method":"item/completed","params":{"threadId":"root","item":{"type":"agentMessage","id":"parent","text":"parent answer"}}}),
            json!({"method":"item/completed","params":{"threadId":"root","item":{"type":"subAgentActivity","id":"s","kind":"started","agentThreadId":"child","agentPath":"/root/research"}}}),
        ];
        for _ in 0..1000 {
            frames.push(json!({"method":"item/agentMessage/delta","params":{"threadId":"child","itemId":"c","delta":"a"}}));
        }
        frames.push(json!({"method":"item/completed","params":{"threadId":"child","item":{"type":"agentMessage","id":"c","text":"child answer"}}}));
        for value in &frames {
            writer
                .append_native_output(
                    NativeAuditChannel::Stdout,
                    "application/json",
                    Uuid::nil(),
                    &serde_json::to_vec(value).unwrap(),
                )
                .unwrap();
        }
        let manifest = writer.close().unwrap();
        let path = root.path().join(&manifest.manifest_relative_path);
        let bundle = crate::runtime::AuditBundle::read(path.parent().unwrap()).unwrap();
        let mapper = provider.mapper();
        let first = bundle.replay(&mapper).unwrap();
        assert_eq!(first.provider_events.len(), frames.len());
        for (raw, expected) in first.provider_events.iter().zip(frames) {
            assert_eq!(raw.payload_json.as_ref(), Some(&expected));
        }
        assert_eq!(first.agent_events.len(), 4);
        assert_eq!(
            first.state.terminal_output.as_ref().unwrap().content,
            "parent answer"
        );
        assert_eq!(
            bundle.replay(&mapper).unwrap().agent_events,
            first.agent_events
        );
    }

    #[test]
    fn checked_in_native_fixtures_replay_for_every_v1_provider() {
        for provider in DirectProvider::ALL {
            let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/native-audit")
                .join(provider.id());
            let bundle = crate::runtime::AuditBundle::read(&directory).unwrap_or_else(|error| {
                panic!("failed to read {} fixture: {error}", provider.id())
            });
            let replay = bundle
                .replay_fixture(&provider.legacy_mapper())
                .unwrap_or_else(|error| {
                    panic!("failed to replay {} fixture: {error}", provider.id())
                });

            assert_eq!(bundle.manifest().provider_id, provider.id());
            assert_eq!(replay.provider_events.len(), 3);
            assert_eq!(replay.agent_events.len(), 3);
            assert_eq!(replay.state.status, AgentRunStatus::Succeeded);
            assert_eq!(replay.state.last_event_sequence, 3);
        }
    }

    #[test]
    fn mapper_v2_does_not_relabel_legacy_audit_bundles() {
        for provider in DirectProvider::ALL {
            assert!(provider.mapper().versions().mapper_version.ends_with(
                if provider == DirectProvider::Codex {
                    "-v3"
                } else {
                    "-v2"
                }
            ));
            assert!(
                provider
                    .semantic_mapper()
                    .versions()
                    .mapper_version
                    .ends_with("-v2")
            );
            assert!(
                provider
                    .legacy_mapper()
                    .versions()
                    .mapper_version
                    .ends_with("-v1")
            );
        }
    }
}
