use std::path::{Component, Path};

use chrono::{DateTime, Utc};
use db::models::{
    agent_runtime::{AgentProviderSessionRecord, AgentRuntimePersistenceError},
    session::{Session, SessionError},
    workspace::{Workspace, WorkspaceKind},
    workspace_repo::WorkspaceRepo,
};
use deployment::Deployment;
use executors::{
    actions::SelectedSkill,
    executors::provider_adapter::DirectProvider,
    profile::{ExecutionMode, ExecutorConfig},
    provider_policy::direct_provider_capability_snapshot,
    runtime::{
        AGENT_REQUEST_PAYLOAD_VERSION, AGENT_REQUEST_SCHEMA_VERSION, AgentCapability,
        AgentRunIntent, AgentRunPort, AgentRunPortError, AgentRunPortSnapshot,
        AgentRunRequestEnvelope, AgentRuntimeMessageRole, CanonicalMessage, CapabilitySnapshot,
        CapabilityState, PROVIDER_SESSION_REFERENCE_SCHEMA_VERSION, ProviderSessionReference,
        RunAttemptMode, RunAttemptRequest, WorkspaceMode, WorkspaceReference,
    },
};
use serde_json::Value;
use services::services::wiki::{self, WikiError};
use sha2::{Digest, Sha256};
use sqlx::{SqlitePool, types::Json};
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

pub(super) struct AgentRunLaunch {
    pub intent: AgentRunIntent,
    pub mode: RunAttemptMode,
    pub prompt: String,
    pub selected_skills: Option<Vec<SelectedSkill>>,
    pub executor_config: ExecutorConfig,
    pub provider_session: Option<ProviderSessionReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentRunDispatch {
    Immediate,
    Reserved,
}

fn initialise_workspace_wikis(
    workspace_path: &Path,
    repo_names: &[String],
    use_workspace_root: bool,
) -> Result<(), WikiError> {
    let workspace_root = std::fs::canonicalize(workspace_path)?;
    let repo_roots = if repo_names.is_empty() {
        if !use_workspace_root {
            return Err(WikiError::InvalidLayout(
                "an LLM Wiki-enabled workspace requires at least one repository".to_string(),
            ));
        }
        vec![workspace_root.clone()]
    } else {
        repo_names
            .iter()
            .map(|name| {
                let path = Path::new(name);
                if path.components().count() != 1
                    || !matches!(path.components().next(), Some(Component::Normal(_)))
                {
                    return Err(WikiError::UnsafePath(name.clone()));
                }
                let root = std::fs::canonicalize(workspace_root.join(path))?;
                if !root.starts_with(&workspace_root) {
                    return Err(WikiError::UnsafePath(root.display().to_string()));
                }
                Ok(root)
            })
            .collect::<Result<Vec<_>, WikiError>>()?
    };

    // Validate every existing Wiki before creating any missing Wiki so one bad
    // repository cannot leave a multi-repository preflight half-applied.
    for repo_root in &repo_roots {
        if repo_root.join(wiki::WIKI_DIR).exists() {
            wiki::load_snapshot(repo_root)?;
        }
    }
    for repo_root in repo_roots {
        wiki::initialise_if_missing(&repo_root, wiki::DEFAULT_OUTPUT_LANGUAGE)?;
    }
    Ok(())
}

async fn prepare_llm_wiki(
    deployment: &DeploymentImpl,
    workspace: &Workspace,
    workspace_path: &Path,
    prompt: &str,
) -> Result<(), ApiError> {
    if !executors::knowledge_skills::contains_wikillm_block(prompt) {
        return Ok(());
    }

    let repo_names = WorkspaceRepo::find_repos_for_workspace(&deployment.db().pool, workspace.id)
        .await?
        .into_iter()
        .map(|repo| repo.name)
        .collect::<Vec<_>>();
    let workspace_path = workspace_path.to_path_buf();
    let use_workspace_root = workspace.is_direct_folder() && repo_names.is_empty();
    tokio::task::spawn_blocking(move || {
        initialise_workspace_wikis(&workspace_path, &repo_names, use_workspace_root)
    })
    .await
    .map_err(|error| ApiError::BadRequest(format!("LLM Wiki preflight failed: {error}")))?
    .map_err(|error| ApiError::BadRequest(format!("LLM Wiki preflight failed: {error}")))
}

pub(super) async fn has_active_agent_run_for_session(
    pool: &SqlitePool,
    session_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM agent_runs
            WHERE session_id = ?
              AND status NOT IN ('succeeded', 'failed', 'cancelled', 'crashed', 'audit_failed')
        )
        "#,
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
}

pub(super) async fn has_active_agent_run_for_workspace(
    pool: &SqlitePool,
    workspace_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM agent_runs
            WHERE workspace_id = ?
              AND status NOT IN ('succeeded', 'failed', 'cancelled', 'crashed', 'audit_failed')
        )
        "#,
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await
}

pub(super) async fn validate_session_executor(
    pool: &SqlitePool,
    session: &Session,
    executor_config: &ExecutorConfig,
) -> Result<(), ApiError> {
    let requested = executor_config.executor.to_string();
    if let Some(expected) = session.executor.as_ref()
        && expected != &requested
    {
        return Err(ApiError::Session(SessionError::ExecutorMismatch {
            expected: expected.clone(),
            actual: requested,
        }));
    }
    if session.executor.is_none() {
        Session::update_executor(pool, session.id, &requested).await?;
    }
    Ok(())
}

pub(super) fn direct_provider(
    executor_config: &ExecutorConfig,
) -> Result<DirectProvider, ApiError> {
    DirectProvider::from_base_agent(executor_config.executor).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "{} is not a V1 direct Agent Runtime provider",
            executor_config.executor
        ))
    })
}

pub(super) async fn latest_provider_session(
    pool: &SqlitePool,
    session_id: Uuid,
    provider: DirectProvider,
    runtime_profile_id: &str,
) -> Result<Option<ProviderSessionReference>, sqlx::Error> {
    let reference = sqlx::query_scalar::<_, Json<ProviderSessionReference>>(
        r#"
        SELECT session_reference
        FROM agent_provider_sessions
        WHERE session_id = ? AND provider_id = ? AND runtime_profile_id = ?
        LIMIT 1
        "#,
    )
    .bind(session_id)
    .bind(provider.id())
    .bind(runtime_profile_id)
    .fetch_optional(pool)
    .await?;
    Ok(reference.map(|reference| reference.0))
}

pub(super) fn explicit_provider_session(
    provider: DirectProvider,
    runtime_profile_id: &str,
    provider_session_id: Option<&str>,
    observed_at: DateTime<Utc>,
) -> Option<ProviderSessionReference> {
    provider_session_id
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(|session_id| ProviderSessionReference {
            schema_version: PROVIDER_SESSION_REFERENCE_SCHEMA_VERSION,
            provider_id: provider.id().to_string(),
            runtime_profile_id: runtime_profile_id.to_string(),
            provider_session_id: session_id.to_string(),
            observed_at,
            metadata: Some(serde_json::json!({ "source": "explicit_native_resume" })),
        })
}

pub(super) fn native_adoption_reference(
    mut reference: ProviderSessionReference,
    executor_config: &ExecutorConfig,
    selected_skills: Option<&Vec<SelectedSkill>>,
    runtime_scope_path: &Path,
    native_source_scope_path: Option<&Path>,
) -> ProviderSessionReference {
    let profile_context = native_adoption_profile_context(executor_config, selected_skills);
    let profile_fingerprint = native_adoption_profile_fingerprint(&profile_context);
    reference.metadata = Some(serde_json::json!({
        "source": "native_adopted",
        "profile_fingerprint": profile_fingerprint,
        "profile_context": profile_context,
        "scope_path": runtime_scope_path.to_string_lossy().to_string(),
        "native_source_scope_path": native_source_scope_path
            .map(|path| path.to_string_lossy().to_string()),
    }));
    reference
}

fn native_adoption_profile_context(
    executor_config: &ExecutorConfig,
    selected_skills: Option<&Vec<SelectedSkill>>,
) -> Value {
    let mut skills = selected_skills.cloned().unwrap_or_default();
    skills.sort_by(|left, right| {
        left.name.cmp(&right.name).then_with(|| {
            left.path
                .to_string_lossy()
                .cmp(&right.path.to_string_lossy())
        })
    });
    serde_json::json!({
        "executor_config": executor_config,
        "selected_skills": skills,
    })
}

fn native_adoption_profile_fingerprint(profile_context: &Value) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(profile_context).expect("profile context serializes"))
    )
}

fn provider_session_source(reference: &ProviderSessionReference) -> Option<&str> {
    reference
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("source"))
        .and_then(Value::as_str)
}

pub(super) fn validate_native_resume_identity(
    provider: DirectProvider,
    provider_session_id: &str,
    scope_path: &Path,
) -> Result<(), ApiError> {
    let provider_uses_authoritative_resume =
        matches!(provider, DirectProvider::Gemini | DirectProvider::OhMyPi);
    let provider_session_id = provider_session_id.trim();
    executors::executors::provider_adapter::validate_native_session_id(
        provider,
        provider_session_id,
    )
    .map_err(ApiError::BadRequest)?;
    if scope_path.as_os_str().is_empty() {
        return Err(ApiError::BadRequest(
            "Native session adoption requires a non-empty working directory scope".to_string(),
        ));
    }

    let preview = super::native_history::get_native_agent_session_preview(
        provider.id(),
        provider_session_id,
        1,
        &[scope_path.to_path_buf()],
        false,
    );
    if preview.is_none() {
        if provider_uses_authoritative_resume {
            // Gemini and Oh My Pi can resume an explicit native id even when
            // their local history directory is unavailable to VK. The native
            // adapter/CLI is authoritative for whether that id exists; do not
            // reject it or silently substitute VK history here.
            return Ok(());
        }
        return Err(ApiError::BadRequest(format!(
            "Native session {provider_session_id:?} was not found in the selected {} working directory",
            provider.id()
        )));
    }
    Ok(())
}

pub(super) async fn validate_session_provider_binding(
    pool: &SqlitePool,
    session_id: Uuid,
    provider: DirectProvider,
    runtime_profile_id: &str,
    requested: Option<&ProviderSessionReference>,
    executor_config: &ExecutorConfig,
    selected_skills: Option<&Vec<SelectedSkill>>,
    requested_runtime_scope_path: Option<&Path>,
) -> Result<(), ApiError> {
    let existing = sqlx::query_as::<_, (String, Json<ProviderSessionReference>)>(
        r#"
        SELECT provider_id, session_reference
        FROM agent_provider_sessions
        WHERE session_id = ?
        LIMIT 1
        "#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?
    .map(|(provider_id, reference)| (provider_id, reference.0));

    if let Some((existing_provider_id, existing)) = existing {
        if existing_provider_id != provider.id() {
            return Err(ApiError::BadRequest(format!(
                "Session is bound to provider {existing_provider_id}; it cannot switch to {}",
                provider.id()
            )));
        }
        if existing.runtime_profile_id != runtime_profile_id {
            return Err(ApiError::BadRequest(format!(
                "Session is bound to runtime profile {}; create a new VK session for {}",
                existing.runtime_profile_id, runtime_profile_id
            )));
        }
        if let Some(requested) = requested
            && existing.provider_session_id != requested.provider_session_id
        {
            return Err(ApiError::BadRequest(format!(
                "Session is already bound to native session {}; it cannot switch to {}",
                existing.provider_session_id, requested.provider_session_id
            )));
        }
        if provider_session_source(&existing) == Some("native_adopted") {
            let expected_scope = existing
                .metadata
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("scope_path"))
                .and_then(Value::as_str);
            if let Some(requested_runtime_scope_path) = requested_runtime_scope_path
                && expected_scope != Some(requested_runtime_scope_path.to_string_lossy().as_ref())
            {
                return Err(ApiError::BadRequest(
                    "Native session working directory changed; create a new VK session".to_string(),
                ));
            }

            let expected = existing
                .metadata
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("profile_fingerprint"))
                .and_then(Value::as_str);
            let actual_context = native_adoption_profile_context(executor_config, selected_skills);
            let actual = native_adoption_profile_fingerprint(&actual_context);
            if expected != Some(actual.as_str()) {
                return Err(ApiError::BadRequest(
                    "Native session runtime profile changed; create a new VK session".to_string(),
                ));
            }
        }
    }

    if let Some(requested) = requested {
        let owner = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT session_id
            FROM agent_provider_sessions
            WHERE provider_id = ? AND provider_session_id = ?
            LIMIT 1
            "#,
        )
        .bind(provider.id())
        .bind(&requested.provider_session_id)
        .fetch_optional(pool)
        .await?;
        if owner.is_some_and(|owner| owner != session_id) {
            return Err(ApiError::BadRequest(format!(
                "Native session {} is already adopted by another VK session",
                requested.provider_session_id
            )));
        }
    }

    Ok(())
}

pub(super) async fn bind_provider_session(
    pool: &SqlitePool,
    session_id: Uuid,
    reference: &ProviderSessionReference,
) -> Result<(), ApiError> {
    AgentProviderSessionRecord::upsert(pool, Uuid::new_v4(), session_id, reference)
        .await
        .map_err(|error| match error {
            AgentRuntimePersistenceError::Database(error) => ApiError::Database(error),
            AgentRuntimePersistenceError::IdentityConflict { key, .. } => {
                ApiError::BadRequest(format!("Native session binding conflict: {key}"))
            }
            other => ApiError::BadRequest(format!("Failed to bind native session: {other}")),
        })
}

pub(super) async fn create_agent_run(
    deployment: &DeploymentImpl,
    session: &Session,
    workspace: &Workspace,
    workspace_path: String,
    launch: AgentRunLaunch,
    dispatch: AgentRunDispatch,
) -> Result<AgentRunPortSnapshot, ApiError> {
    let provider = direct_provider(&launch.executor_config)?;
    let runtime_profile_id = launch.executor_config.profile_id().cache_key();
    let capability_snapshot =
        direct_provider_capability_snapshot(provider, runtime_profile_id.clone());
    validate_execution_config(&launch.executor_config, &capability_snapshot)?;
    validate_required_capabilities(
        launch.intent,
        launch.provider_session.is_some(),
        &capability_snapshot,
    )?;
    prepare_llm_wiki(
        deployment,
        workspace,
        Path::new(&workspace_path),
        &launch.prompt,
    )
    .await?;

    let request_id = Uuid::new_v4();
    let agent_run_id = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let run_attempt_id = Uuid::new_v4();
    let correlation_id = Uuid::new_v4();
    let created_at = Utc::now();
    let idempotency_key = format!("session:{}:request:{request_id}:create", session.id);
    let workspace = WorkspaceReference {
        workspace_id: workspace.id,
        mode: match workspace.workspace_kind {
            WorkspaceKind::DirectFolder => WorkspaceMode::SharedWorkspace,
            WorkspaceKind::Worktree => WorkspaceMode::IsolatedWorktree,
        },
        path: workspace_path,
    };
    let request = AgentRunRequestEnvelope {
        schema_version: AGENT_REQUEST_SCHEMA_VERSION,
        payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
        request_id,
        idempotency_key: idempotency_key.clone(),
        session_id: session.id,
        agent_run_id,
        turn_id,
        correlation_id,
        intent: launch.intent,
        runtime_profile_id: runtime_profile_id.clone(),
        provider_id: provider.id().to_string(),
        workspace: workspace.clone(),
        input: CanonicalMessage {
            message_id: Uuid::new_v4(),
            role: AgentRuntimeMessageRole::User,
            content: launch.prompt,
        },
        created_at,
    };
    let attempt = RunAttemptRequest {
        schema_version: AGENT_REQUEST_SCHEMA_VERSION,
        payload_version: AGENT_REQUEST_PAYLOAD_VERSION,
        request_id,
        idempotency_key: format!("{idempotency_key}:attempt:1"),
        session_id: session.id,
        agent_run_id,
        turn_id,
        run_attempt_id,
        attempt_number: 1,
        correlation_id,
        mode: launch.mode,
        transport: provider.transport(),
        runtime_profile_id,
        provider_id: provider.id().to_string(),
        workspace,
        capability_snapshot,
        executor_config: launch.executor_config,
        selected_skills: launch.selected_skills.filter(|skills| !skills.is_empty()),
        reset_to_message_id: None,
        provider_session: launch.provider_session,
        created_at,
    };

    let port = deployment.agent_run_port();
    let persisted_agent_run_id = match dispatch {
        AgentRunDispatch::Immediate => port.create(request, attempt).await,
        AgentRunDispatch::Reserved => port.reserve(request, attempt).await,
    }
    .map_err(agent_run_port_error)?;
    port.query(persisted_agent_run_id)
        .await
        .map_err(agent_run_port_error)
}

fn validate_execution_config(
    config: &ExecutorConfig,
    snapshot: &CapabilitySnapshot,
) -> Result<(), ApiError> {
    let mode = config.execution_mode.unwrap_or_else(|| {
        if config.permission_policy == Some(executors::model_selector::PermissionPolicy::Plan) {
            ExecutionMode::Plan
        } else {
            ExecutionMode::Code
        }
    });
    if matches!(mode, ExecutionMode::Goal | ExecutionMode::PlanWithGoal) {
        require_native_capability(snapshot, AgentCapability::Goal)?;
    }
    if config.goal_token_budget == Some(0) {
        return Err(ApiError::BadRequest(
            "Goal token budget must be a positive integer".to_string(),
        ));
    }
    if !matches!(mode, ExecutionMode::Goal | ExecutionMode::PlanWithGoal)
        && (config.goal_token_budget.is_some() || config.goal_max_concurrent_agents.is_some())
    {
        return Err(ApiError::BadRequest(
            "Goal options require Goal or Plan with Goal mode".to_string(),
        ));
    }
    Ok(())
}

fn validate_required_capabilities(
    intent: AgentRunIntent,
    has_provider_session: bool,
    snapshot: &CapabilitySnapshot,
) -> Result<(), ApiError> {
    if matches!(intent, AgentRunIntent::FollowUp) {
        if !has_provider_session {
            return Err(ApiError::BadRequest(
                "A canonical follow-up requires an observed or explicitly selected native session"
                    .to_string(),
            ));
        }
        require_native_capability(snapshot, AgentCapability::SessionResume)?;
    }
    if matches!(intent, AgentRunIntent::Review) {
        require_native_capability(snapshot, AgentCapability::Review)?;
    }
    Ok(())
}

fn require_native_capability(
    snapshot: &CapabilitySnapshot,
    capability: AgentCapability,
) -> Result<(), ApiError> {
    match snapshot.resolve(capability) {
        CapabilityState::Native => Ok(()),
        state => Err(ApiError::BadRequest(format!(
            "Provider {} cannot perform {capability:?}: frozen capability is {state:?}",
            snapshot.provider_id
        ))),
    }
}

pub(super) fn agent_run_port_error(error: AgentRunPortError) -> ApiError {
    match error {
        AgentRunPortError::Rejected(message) => ApiError::BadRequest(message),
        AgentRunPortError::NotFound(agent_run_id) => {
            ApiError::BadRequest(format!("Canonical AgentRun {agent_run_id} was not found"))
        }
        AgentRunPortError::Unavailable(message) => ApiError::BadGateway(message),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use executors::{
        actions::SelectedSkill,
        executors::{BaseCodingAgent, provider_adapter::DirectProvider},
        profile::ExecutorConfig,
        runtime::{AgentRunIntent, RunAttemptMode},
    };

    use super::{
        direct_provider, explicit_provider_session, initialise_workspace_wikis,
        native_adoption_profile_context, native_adoption_profile_fingerprint,
        native_adoption_reference, validate_native_resume_identity,
    };

    async fn active_run_test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        sqlx::query(
            "CREATE TABLE agent_runs (session_id BLOB NOT NULL, workspace_id BLOB NOT NULL, status TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .expect("create test agent_runs table");
        pool
    }

    async fn provider_binding_test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        sqlx::query(
            r#"
            CREATE TABLE agent_provider_sessions (
                session_id BLOB NOT NULL,
                provider_id TEXT NOT NULL,
                runtime_profile_id TEXT NOT NULL,
                provider_session_id TEXT NOT NULL,
                session_reference TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create provider binding table");
        pool
    }

    #[test]
    fn direct_provider_mapping_covers_the_four_v1_runtimes() {
        let cases = [
            (BaseCodingAgent::Gemini, DirectProvider::Gemini),
            (BaseCodingAgent::Codex, DirectProvider::Codex),
            (BaseCodingAgent::ClaudeCode, DirectProvider::ClaudeCode),
            (BaseCodingAgent::OhMyPi, DirectProvider::OhMyPi),
        ];
        for (agent, expected) in cases {
            assert_eq!(
                direct_provider(&ExecutorConfig::new(agent)).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn wiki_preflight_initialises_each_workspace_repository() {
        let workspace = tempfile::tempdir().unwrap();
        for name in ["frontend", "backend"] {
            std::fs::create_dir(workspace.path().join(name)).unwrap();
        }

        initialise_workspace_wikis(
            workspace.path(),
            &["frontend".to_string(), "backend".to_string()],
            false,
        )
        .unwrap();

        for name in ["frontend", "backend"] {
            let snapshot =
                services::services::wiki::load_snapshot(&workspace.path().join(name)).unwrap();
            assert!(snapshot.exists);
            assert_eq!(
                snapshot.config.unwrap().output_language,
                services::services::wiki::DEFAULT_OUTPUT_LANGUAGE
            );
        }
    }

    #[test]
    fn wiki_preflight_uses_workspace_root_for_a_repo_less_direct_folder() {
        let workspace = tempfile::tempdir().unwrap();
        initialise_workspace_wikis(workspace.path(), &[], true).unwrap();

        assert!(
            services::services::wiki::load_snapshot(workspace.path())
                .unwrap()
                .exists
        );
    }

    #[test]
    fn wiki_preflight_refuses_invalid_existing_data() {
        let workspace = tempfile::tempdir().unwrap();
        let repo = workspace.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(repo.join(services::services::wiki::WIKI_DIR)).unwrap();

        assert!(
            initialise_workspace_wikis(workspace.path(), &["repo".to_string()], false).is_err()
        );
        assert!(!repo.join(".llm-wiki/config.toml").exists());
    }

    #[test]
    fn wiki_preflight_validates_all_repositories_before_writing() {
        let workspace = tempfile::tempdir().unwrap();
        let clean = workspace.path().join("clean");
        let invalid = workspace.path().join("invalid");
        std::fs::create_dir(&clean).unwrap();
        std::fs::create_dir(&invalid).unwrap();
        std::fs::create_dir(invalid.join(services::services::wiki::WIKI_DIR)).unwrap();

        assert!(
            initialise_workspace_wikis(
                workspace.path(),
                &["clean".to_string(), "invalid".to_string()],
                false,
            )
            .is_err()
        );
        assert!(!clean.join(".llm-wiki").exists());
    }

    #[test]
    fn wiki_preflight_rejects_a_repo_less_worktree_workspace() {
        let workspace = tempfile::tempdir().unwrap();

        assert!(initialise_workspace_wikis(workspace.path(), &[], false).is_err());
        assert!(!workspace.path().join(".llm-wiki").exists());
    }

    #[test]
    fn explicit_native_resume_reference_is_trimmed_and_provider_scoped() {
        let observed_at = chrono::Utc::now();
        let reference = explicit_provider_session(
            DirectProvider::Codex,
            "CODEX:DEFAULT",
            Some("  native-thread  "),
            observed_at,
        )
        .expect("provider session reference");

        assert_eq!(reference.provider_session_id, "native-thread");
        assert_eq!(reference.provider_id, "codex");
        assert_eq!(reference.runtime_profile_id, "CODEX:DEFAULT");
        assert_eq!(reference.observed_at, observed_at);
    }

    #[test]
    fn explicit_native_resume_reference_rejects_blank_ids() {
        assert!(
            explicit_provider_session(
                DirectProvider::ClaudeCode,
                "CLAUDE_CODE",
                Some("  "),
                chrono::Utc::now(),
            )
            .is_none()
        );
    }

    #[test]
    fn native_adoption_reference_records_provenance_and_profile_context() {
        let config = ExecutorConfig::new(BaseCodingAgent::Codex);
        let skills = vec![SelectedSkill {
            name: "review".to_string(),
            path: PathBuf::from(".codex/skills/review"),
        }];
        let reference = native_adoption_reference(
            explicit_provider_session(
                DirectProvider::Codex,
                &config.profile_id().cache_key(),
                Some("native-session"),
                chrono::Utc::now(),
            )
            .unwrap(),
            &config,
            Some(&skills),
            std::path::Path::new("C:\\workspace"),
            Some(std::path::Path::new("C:\\source")),
        );

        let metadata = reference.metadata.expect("adoption metadata");
        assert_eq!(metadata["source"], "native_adopted");
        assert!(metadata["profile_fingerprint"].as_str().is_some());
        assert_eq!(metadata["scope_path"], "C:\\workspace");
        assert_eq!(metadata["native_source_scope_path"], "C:\\source");
    }

    #[test]
    fn native_adoption_profile_fingerprint_is_independent_of_skill_order() {
        let config = ExecutorConfig::new(BaseCodingAgent::Codex);
        let first = vec![
            SelectedSkill {
                name: "zeta".to_string(),
                path: PathBuf::from(".codex/skills/zeta"),
            },
            SelectedSkill {
                name: "alpha".to_string(),
                path: PathBuf::from(".codex/skills/alpha"),
            },
        ];
        let second = vec![first[1].clone(), first[0].clone()];
        assert_eq!(
            native_adoption_profile_fingerprint(&native_adoption_profile_context(
                &config,
                Some(&first)
            )),
            native_adoption_profile_fingerprint(&native_adoption_profile_context(
                &config,
                Some(&second)
            ))
        );
    }

    #[tokio::test]
    async fn native_adoption_binding_rejects_provider_profile_scope_and_id_changes() {
        let pool = provider_binding_test_pool().await;
        let session_id = uuid::Uuid::new_v4();
        let config = ExecutorConfig::new(BaseCodingAgent::Codex);
        let skills = vec![SelectedSkill {
            name: "review".to_string(),
            path: PathBuf::from(".codex/skills/review"),
        }];
        let reference = native_adoption_reference(
            explicit_provider_session(
                DirectProvider::Codex,
                &config.profile_id().cache_key(),
                Some("native-session"),
                chrono::Utc::now(),
            )
            .unwrap(),
            &config,
            Some(&skills),
            std::path::Path::new("C:/vk-worktree"),
            Some(std::path::Path::new("C:/native-source")),
        );
        sqlx::query(
            "INSERT INTO agent_provider_sessions (session_id, provider_id, runtime_profile_id, provider_session_id, session_reference) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind("codex")
        .bind(&reference.runtime_profile_id)
        .bind(&reference.provider_session_id)
        .bind(serde_json::to_string(&reference).unwrap())
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            super::validate_session_provider_binding(
                &pool,
                session_id,
                DirectProvider::Codex,
                &config.profile_id().cache_key(),
                Some(&reference),
                &config,
                Some(&skills),
                Some(std::path::Path::new("C:/vk-worktree")),
            )
            .await
            .is_ok()
        );

        let scope_error = super::validate_session_provider_binding(
            &pool,
            session_id,
            DirectProvider::Codex,
            &config.profile_id().cache_key(),
            Some(&reference),
            &config,
            Some(&skills),
            Some(std::path::Path::new("C:/other-worktree")),
        )
        .await
        .expect_err("scope changes must fail");
        assert!(
            scope_error
                .to_string()
                .contains("working directory changed")
        );

        let mut changed_config = config.clone();
        changed_config.model_id = Some("different-model".to_string());
        let profile_error = super::validate_session_provider_binding(
            &pool,
            session_id,
            DirectProvider::Codex,
            &config.profile_id().cache_key(),
            Some(&reference),
            &changed_config,
            Some(&skills),
            Some(std::path::Path::new("C:/vk-worktree")),
        )
        .await
        .expect_err("profile changes must fail");
        assert!(
            profile_error
                .to_string()
                .contains("runtime profile changed")
        );

        let provider_error = super::validate_session_provider_binding(
            &pool,
            session_id,
            DirectProvider::ClaudeCode,
            "CLAUDE_CODE",
            None,
            &ExecutorConfig::new(BaseCodingAgent::ClaudeCode),
            None,
            Some(std::path::Path::new("C:/vk-worktree")),
        )
        .await
        .expect_err("provider changes must fail");
        assert!(provider_error.to_string().contains("cannot switch"));
    }

    #[test]
    fn canonical_initial_and_resume_modes_remain_distinct() {
        assert_ne!(AgentRunIntent::Initial, AgentRunIntent::FollowUp);
        assert_ne!(RunAttemptMode::Launch, RunAttemptMode::Resume);
    }

    #[test]
    fn explicit_gemini_and_omp_resume_ids_remain_provider_authoritative() {
        let scope = std::path::Path::new("C:/native-source");
        for provider in [DirectProvider::Gemini, DirectProvider::OhMyPi] {
            assert!(
                validate_native_resume_identity(
                    provider,
                    "provider-owned-id-that-is-not-locally-readable",
                    scope,
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn codex_and_claude_resume_ids_still_require_discoverable_history() {
        let scope = std::path::Path::new("C:/native-source");
        for provider in [DirectProvider::Codex, DirectProvider::ClaudeCode] {
            assert!(
                validate_native_resume_identity(
                    provider,
                    "definitely-not-a-local-session-id",
                    scope,
                )
                .is_err()
            );
        }
    }

    #[tokio::test]
    async fn active_run_guards_use_canonical_status_for_session_and_workspace() {
        let pool = active_run_test_pool().await;
        let session_id = uuid::Uuid::new_v4();
        let workspace_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO agent_runs (session_id, workspace_id, status) VALUES (?, ?, ?)")
            .bind(session_id)
            .bind(workspace_id)
            .bind("running")
            .execute(&pool)
            .await
            .expect("insert running AgentRun");

        assert!(
            super::has_active_agent_run_for_session(&pool, session_id)
                .await
                .unwrap()
        );
        assert!(
            super::has_active_agent_run_for_workspace(&pool, workspace_id)
                .await
                .unwrap()
        );

        sqlx::query("UPDATE agent_runs SET status = 'succeeded'")
            .execute(&pool)
            .await
            .expect("terminalize AgentRun");
        assert!(
            !super::has_active_agent_run_for_session(&pool, session_id)
                .await
                .unwrap()
        );
        assert!(
            !super::has_active_agent_run_for_workspace(&pool, workspace_id)
                .await
                .unwrap()
        );
    }
}
