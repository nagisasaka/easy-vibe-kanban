//! Public management boundary. Native adapters retain the full values for
//! precedence, targeted writes and verification. Discovery is not a raw-file
//! read, and a summary can never be submitted as a replacement configuration.
use std::sync::Mutex;

use super::*;

// As in upstream 76a86da, serialize the infrequent profile read/check/write
// operation, not the provider runtime. File revisions still detect external
// edits; this is not a lock against arbitrary external filesystem writers.
static PROFILE_WRITES: Mutex<()> = Mutex::new(());

fn public_setting(key: &SettingKey) -> bool {
    // Deliberate allowlist: JSON, environment, hooks, command lines and unknown
    // provider extensions may contain secrets, even without a "token" key.
    matches!(
        key.id().as_str(),
        "common.model"
            | "common.reasoning"
            | "common.permission_mode"
            | "common.sandbox_enabled"
            | "common.approval_policy"
            | "common.sandbox_mode"
            | "codex.web_search"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SettingsSnapshotView {
    #[serde(flatten)]
    pub snapshot: SettingsSnapshot,
    /// Values are absent, not editable mask strings. Sources still identify
    /// whether a value is configured; omission from a patch means preserve.
    pub withheld_setting_keys: Vec<String>,
}

impl From<SettingsSnapshot> for SettingsSnapshotView {
    fn from(mut snapshot: SettingsSnapshot) -> Self {
        let mut withheld_setting_keys = Vec::new();
        for setting in &mut snapshot.effective_settings {
            if !public_setting(&setting.key) {
                withheld_setting_keys.push(setting.key.id());
                setting.effective_value = None;
                for source in &mut setting.sources {
                    source.value = Value::Null;
                }
                setting.warnings.clear();
            }
        }
        for file in &mut snapshot.native_files {
            file.raw_content = None;
            if file.error.is_some() {
                file.error = Some(
                    "Native file could not be parsed; inspect it explicitly before editing.".into(),
                );
            }
        }
        for node in &mut snapshot.unknown_native_nodes {
            node.value = Value::Null;
        }
        for issue in &mut snapshot.errors {
            issue.message = "Native settings discovery failed; refresh or explicitly inspect the indicated file.".into();
        }
        Self {
            snapshot,
            withheld_setting_keys,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentSettingsInventoryView {
    pub providers: Vec<SettingsSnapshotView>,
    pub errors: Vec<AgentSettingsProviderError>,
}

impl From<AgentSettingsInventory> for AgentSettingsInventoryView {
    fn from(inventory: AgentSettingsInventory) -> Self {
        Self {
            providers: inventory.providers.into_iter().map(Into::into).collect(),
            errors: inventory
                .errors
                .into_iter()
                .map(|mut error| {
                    error.message =
                        "Native settings discovery failed. Inspect the provider files explicitly."
                            .into();
                    error
                })
                .collect(),
        }
    }
}

/// Normal diff responses describe the operation, not unrelated full-file values.
pub fn public_diff(mut diff: SettingsDiff) -> SettingsDiff {
    for file in &mut diff.files {
        file.before = "Observed native revision (values withheld)".into();
        file.after = if file.changed {
            "Requested settings will be applied; unrelated native values are preserved."
        } else {
            "No change."
        }
        .into();
    }
    diff
}

pub fn public_error(
    error: AgentSettingError,
    provider: Option<AgentSettingsProvider>,
) -> AgentSettingOperationError {
    let mut result = error.operation_error(provider, None, None);
    // Parser errors can quote whole native source lines. Preserve the code and
    // recovery instruction, never a parser's input excerpt.
    result.message = match result.code {
        AgentSettingErrorCode::StaleRevision => {
            "Configuration changed since it was read. Refresh before retrying."
        }
        AgentSettingErrorCode::NotFound => {
            "The requested native setting or profile no longer exists."
        }
        AgentSettingErrorCode::InvalidRequest => {
            "Invalid settings request or missing explicit confirmation."
        }
        AgentSettingErrorCode::ValidationFailed => {
            "The requested value failed provider validation."
        }
        AgentSettingErrorCode::InvalidConfiguration => {
            "The native configuration could not be parsed."
        }
        AgentSettingErrorCode::Unsupported => "The provider does not support this operation.",
        AgentSettingErrorCode::UnsafePath => "The requested native path is unsafe.",
        AgentSettingErrorCode::Io => "Could not access the native settings file.",
        AgentSettingErrorCode::RollbackFailed => {
            "Settings rollback failed. Inspect the native files before retrying."
        }
        AgentSettingErrorCode::VerificationFailed => {
            "Settings verification failed. Inspect the native files before retrying."
        }
    }
    .into();
    result
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReadNativeSettingsRequest {
    pub provider: AgentSettingsProvider,
    pub project_path: Option<String>,
    pub file_id: String,
    pub expected_revision: String,
    pub confirmed_sensitive_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NativeSettingsEditRequest {
    pub patch: NativeFilePatch,
    pub confirmed_sensitive_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ConfigProfileView {
    pub id: Uuid,
    pub provider: AgentSettingsProvider,
    pub executor_profile: ExecutorProfileId,
    pub name: String,
    pub schema_version: u16,
    pub updated_at: DateTime<Utc>,
    pub revision: String,
    pub setting_keys: Vec<String>,
    pub provider_extension_count: usize,
    pub environment_count: usize,
    pub custom_arg_count: usize,
}

impl From<ConfigProfile> for ConfigProfileView {
    fn from(profile: ConfigProfile) -> Self {
        // The typed profile was read from JSON; serialization is infallible.
        let revision = revision_bytes(&serde_json::to_vec(&profile).expect("profile JSON"));
        Self {
            id: profile.id,
            provider: profile.provider,
            executor_profile: profile.executor_profile,
            name: profile.name,
            schema_version: profile.schema_version,
            updated_at: profile.updated_at,
            revision,
            setting_keys: profile.setting_overrides.keys().cloned().collect(),
            provider_extension_count: profile.provider_extensions.len(),
            environment_count: profile.environment.len(),
            custom_arg_count: profile.custom_args.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProfileReference {
    pub id: Uuid,
    pub expected_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CopyProfileWriteRequest {
    pub source: ProfileReference,
    pub target_provider: AgentSettingsProvider,
    pub target_executor_profile: ExecutorProfileId,
    pub target_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProfileCopyPreviewView {
    pub request: CopyProfileWriteRequest,
    pub compatible_keys: Vec<String>,
    pub skipped_keys: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum WriteConfigProfileRequest {
    /// Capture on the host, including undisclosed values. An empty operations
    /// list preserves all effective values, not an empty/redacted UI projection.
    Capture {
        patch: SettingsPatch,
        executor_profile: ExecutorProfileId,
        name: String,
    },
    Rename {
        source: ProfileReference,
        name: String,
    },
    Duplicate {
        source: ProfileReference,
        name: String,
    },
    Copy {
        request: CopyProfileWriteRequest,
    },
    Delete {
        source: ProfileReference,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ProfileApplyRequest {
    pub reference: ProfileReference,
    pub project_path: Option<String>,
    pub scope: SettingScope,
    pub expected_file_revisions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ConfirmProfileApplyRequest {
    pub preview: ProfileApplyRequest,
    pub confirmed: bool,
}

impl AgentSettingsService {
    pub fn read_native_settings(
        &self,
        request: ReadNativeSettingsRequest,
    ) -> Result<NativeConfigFile, AgentSettingError> {
        if !request.confirmed_sensitive_read {
            return Err(AgentSettingError::InvalidRequest(
                "explicit sensitive read required".into(),
            ));
        }
        let manager = self.manager(
            request.provider,
            request.project_path.as_deref().map(Path::new),
        );
        // Discover uses the adapter's allowlisted file IDs and path protections,
        // including invalid-but-readable files supported by advanced editing.
        let snapshot = manager.discover()?;
        let file = snapshot
            .native_files
            .into_iter()
            .find(|file| file.id == request.file_id)
            .ok_or_else(|| AgentSettingError::NotFound(request.file_id.clone()))?;
        if file.revision.as_deref() != Some(&request.expected_revision) {
            return Err(AgentSettingError::StaleRevision);
        }
        Ok(file)
    }

    fn checked_profile(
        &self,
        reference: &ProfileReference,
    ) -> Result<ConfigProfile, AgentSettingError> {
        let profile = self
            .load_profile_store()?
            .profiles
            .into_iter()
            .find(|p| p.id == reference.id)
            .ok_or_else(|| AgentSettingError::NotFound(reference.id.to_string()))?;
        if ConfigProfileView::from(profile.clone()).revision != reference.expected_revision {
            return Err(AgentSettingError::StaleRevision);
        }
        Ok(profile)
    }

    fn checked_profile_copy(
        &self,
        request: &CopyProfileWriteRequest,
    ) -> Result<ProfileCopyPreview, AgentSettingError> {
        let source = self.checked_profile(&request.source)?;
        let preview = self.copy_profile_preview(CopyProfilePreviewRequest {
            id: source.id,
            target_provider: request.target_provider,
            target_executor_profile: request.target_executor_profile.clone(),
            target_name: request.target_name.clone(),
        })?;
        // An external editor between the two reads must not change the confirmed source.
        self.checked_profile(&request.source)?;
        Ok(preview)
    }

    pub fn preview_profile_copy_view(
        &self,
        request: CopyProfileWriteRequest,
    ) -> Result<ProfileCopyPreviewView, AgentSettingError> {
        let _guard = PROFILE_WRITES
            .lock()
            .map_err(|_| AgentSettingError::VerificationFailed("profile lock".into()))?;
        let preview = self.checked_profile_copy(&request)?;
        Ok(ProfileCopyPreviewView {
            request,
            compatible_keys: preview.compatible_keys,
            skipped_keys: preview.skipped_keys,
            warnings: preview.warnings,
        })
    }

    pub fn write_profile(
        &self,
        request: WriteConfigProfileRequest,
    ) -> Result<Option<ConfigProfileView>, AgentSettingError> {
        let _guard = PROFILE_WRITES
            .lock()
            .map_err(|_| AgentSettingError::VerificationFailed("profile lock".into()))?;
        let mut store = self.load_profile_store()?;
        let before = profile_store_bytes(&self.profile_store_path)?;
        // Detect a changed store even if the selected profile itself is unchanged.
        if serde_json::to_value(&store).ok()
            != serde_json::to_value(self.load_profile_store()?).ok()
        {
            return Err(AgentSettingError::StaleRevision);
        }
        let mut profile = match request {
            WriteConfigProfileRequest::Capture {
                patch,
                executor_profile,
                name,
            } => {
                let snapshot = self
                    .manager(patch.provider, patch.project_path.as_deref().map(Path::new))
                    .discover()?;
                for file in &snapshot.native_files {
                    require_expected_revision(
                        &patch.expected_file_revisions,
                        &file.id,
                        file.revision.as_deref().unwrap_or("missing"),
                    )?;
                }
                if !snapshot.errors.is_empty()
                    || snapshot
                        .native_files
                        .iter()
                        .any(|f| f.parse_status == NativeParseStatus::Invalid)
                {
                    return Err(AgentSettingError::InvalidConfiguration(
                        "cannot capture invalid settings".into(),
                    ));
                }
                let mut values: BTreeMap<_, _> = snapshot
                    .effective_settings
                    .into_iter()
                    .filter_map(|s| s.effective_value.map(|v| (s.key.id(), v)))
                    .collect();
                for operation in &patch.operations {
                    match operation {
                        SettingOperation::Set { key, value, .. } => {
                            values.insert(key.id(), value.clone());
                        }
                        SettingOperation::Unset { key, .. } => {
                            values.remove(&key.id());
                        }
                    }
                }
                let storable: BTreeSet<_> = snapshot
                    .descriptors
                    .iter()
                    .filter(|d| d.capabilities.profile_storable)
                    .map(|d| d.key.id())
                    .collect();
                values.retain(|key, _| storable.contains(key));
                let environment = values
                    .get("common.environment")
                    .and_then(Value::as_object)
                    .map(|v| {
                        v.iter()
                            .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default();
                ConfigProfile {
                    id: Uuid::new_v4(),
                    provider: patch.provider,
                    executor_profile,
                    name,
                    schema_version: SETTINGS_PROFILE_STORE_VERSION,
                    setting_overrides: values,
                    provider_extensions: BTreeMap::new(),
                    environment,
                    custom_args: Vec::new(),
                    updated_at: Utc::now(),
                }
            }
            WriteConfigProfileRequest::Rename { source, name } => {
                let mut profile = self.checked_profile(&source)?;
                profile.name = name;
                profile
            }
            WriteConfigProfileRequest::Duplicate { source, name } => {
                let mut profile = self.checked_profile(&source)?;
                profile.id = Uuid::new_v4();
                profile.name = name;
                profile
            }
            WriteConfigProfileRequest::Copy { request } => {
                self.checked_profile_copy(&request)?.profile
            }
            WriteConfigProfileRequest::Delete { source } => {
                self.checked_profile(&source)?;
                store.profiles.retain(|p| p.id != source.id);
                self.save_profile_store_checked(&store, before)?;
                return Ok(None);
            }
        };
        validate_profile(
            &profile,
            &self.manager(profile.provider, None).descriptors(),
        )?;
        profile.updated_at = Utc::now();
        profile.schema_version = SETTINGS_PROFILE_STORE_VERSION;
        store.profiles.retain(|p| p.id != profile.id);
        store.profiles.push(profile.clone());
        self.save_profile_store_checked(&store, before)?;
        Ok(Some(profile.into()))
    }

    fn save_profile_store_checked(
        &self,
        store: &ConfigProfileStore,
        before: Option<Vec<u8>>,
    ) -> Result<(), AgentSettingError> {
        if profile_store_bytes(&self.profile_store_path)? != before {
            return Err(AgentSettingError::StaleRevision);
        }
        self.save_profile_store(store)
    }

    fn public_profile_patch(
        &self,
        request: &ProfileApplyRequest,
    ) -> Result<SettingsPatch, AgentSettingError> {
        let profile = self.checked_profile(&request.reference)?;
        Ok(profile_patch(
            profile,
            &ProfileApplyPreviewRequest {
                id: request.reference.id,
                project_path: request.project_path.clone(),
                scope: request.scope,
                expected_file_revisions: request.expected_file_revisions.clone(),
            },
        ))
    }

    pub fn preview_profile_apply_view(
        &self,
        request: &ProfileApplyRequest,
    ) -> Result<SettingsDiff, AgentSettingError> {
        self.diff(&self.public_profile_patch(request)?)
            .map(public_diff)
    }

    pub fn apply_profile_view(
        &self,
        request: ConfirmProfileApplyRequest,
    ) -> Result<SettingsSnapshotView, AgentSettingError> {
        let _guard = PROFILE_WRITES
            .lock()
            .map_err(|_| AgentSettingError::VerificationFailed("profile lock".into()))?;
        let patch = self.public_profile_patch(&request.preview)?;
        self.apply(ApplySettingsRequest {
            patch,
            confirmed: request.confirmed,
        })
        .map(Into::into)
    }
}

fn profile_store_bytes(path: &Path) -> Result<Option<Vec<u8>>, AgentSettingError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::executors::BaseCodingAgent;

    fn fixture() -> (tempfile::TempDir, AgentSettingsService, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let file = home.join(".codex/config.toml");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "model = 'safe-model'\nmodel_reasoning_effort = 'max'\nprivate_extra = 'unknown-secret'\n[shell_environment_policy.set]\nTOKEN = 'env-secret'\n").unwrap();
        let service = AgentSettingsService::new(home, root.path().join("profiles.json"));
        (root, service, file)
    }

    #[test]
    fn rendered_edit_and_rollback_do_not_overwrite_an_external_writer() {
        let (_root, service, file) = fixture();
        let manager = service.manager(AgentSettingsProvider::Codex, None);
        let native = snapshot(&service)
            .native_files
            .into_iter()
            .find(|f| f.exists)
            .unwrap();
        let rendered = manager
            .render_patch(&SettingsPatch {
                provider: AgentSettingsProvider::Codex,
                project_path: None,
                expected_file_revisions: BTreeMap::from([(native.id, native.revision.unwrap())]),
                operations: vec![SettingOperation::Set {
                    key: SettingKey::new("common", "model"),
                    scope: SettingScope::User,
                    value: json!("changed"),
                }],
            })
            .unwrap();
        fs::write(&file, "model='external-edit'\n").unwrap();
        let before = fs::read(&file).unwrap();
        assert!(matches!(
            write_all_or_restore(&rendered),
            Err(AgentSettingError::StaleRevision)
        ));
        assert_eq!(fs::read(&file).unwrap(), before);
        assert!(matches!(
            rollback_files(&rendered),
            Err(AgentSettingError::RollbackFailed(_))
        ));
        assert_eq!(fs::read(&file).unwrap(), before);
    }

    fn snapshot(service: &AgentSettingsService) -> SettingsSnapshot {
        service
            .manager(AgentSettingsProvider::Codex, None)
            .discover()
            .unwrap()
    }

    fn capture(service: &AgentSettingsService) -> ConfigProfileView {
        let snapshot = snapshot(service);
        service
            .write_profile(WriteConfigProfileRequest::Capture {
                patch: SettingsPatch {
                    provider: snapshot.provider,
                    project_path: None,
                    operations: vec![],
                    expected_file_revisions: snapshot
                        .native_files
                        .iter()
                        .map(|f| (f.id.clone(), f.revision.clone().unwrap()))
                        .collect(),
                },
                executor_profile: ExecutorProfileId::new(BaseCodingAgent::Codex),
                name: "Capture".into(),
            })
            .unwrap()
            .unwrap()
    }

    fn reference(profile: &ConfigProfileView) -> ProfileReference {
        ProfileReference {
            id: profile.id,
            expected_revision: profile.revision.clone(),
        }
    }

    #[test]
    fn discovery_and_diff_never_send_secret_values_but_explicit_read_is_revision_checked() {
        let (_root, service, file) = fixture();
        let original = snapshot(&service);
        let view: SettingsSnapshotView = original.clone().into();
        let encoded = serde_json::to_string(&view).unwrap();
        assert!(!encoded.contains("env-secret"));
        assert!(!encoded.contains("unknown-secret"));
        assert!(!encoded.contains("raw_content"));
        assert!(encoded.contains("safe-model"));
        assert!(encoded.contains("max"));
        assert!(
            view.withheld_setting_keys
                .contains(&"common.environment".into())
        );
        let native = original.native_files.iter().find(|f| f.exists).unwrap();
        let mut request = ReadNativeSettingsRequest {
            provider: original.provider,
            project_path: None,
            file_id: native.id.clone(),
            expected_revision: native.revision.clone().unwrap(),
            confirmed_sensitive_read: false,
        };
        assert!(service.read_native_settings(request.clone()).is_err());
        request.confirmed_sensitive_read = true;
        assert!(
            service
                .read_native_settings(request.clone())
                .unwrap()
                .raw_content
                .unwrap()
                .contains("env-secret")
        );
        let patch = SettingsPatch {
            provider: original.provider,
            project_path: None,
            expected_file_revisions: BTreeMap::from([(
                native.id.clone(),
                request.expected_revision.clone(),
            )]),
            operations: vec![SettingOperation::Set {
                key: SettingKey::new("common", "model"),
                scope: SettingScope::User,
                value: json!("new-model"),
            }],
        };
        let diff = serde_json::to_string(&public_diff(service.diff(&patch).unwrap())).unwrap();
        assert!(!diff.contains("env-secret"));
        let applied: SettingsSnapshotView = service
            .apply(ApplySettingsRequest {
                patch,
                confirmed: true,
            })
            .unwrap()
            .into();
        assert!(
            !serde_json::to_string(&applied)
                .unwrap()
                .contains("env-secret")
        );
        let bytes = fs::read_to_string(file).unwrap();
        assert!(bytes.contains("env-secret") && bytes.contains("unknown-secret"));
        assert!(matches!(
            service.read_native_settings(request),
            Err(AgentSettingError::StaleRevision)
        ));
    }

    #[test]
    fn profile_capture_rename_duplicate_and_copy_preserve_unread_values_without_returning_them() {
        let (_root, service, _file) = fixture();
        let created = capture(&service);
        let saved = service.checked_profile(&reference(&created)).unwrap();
        assert_eq!(
            saved.setting_overrides["common.environment"]["TOKEN"],
            "env-secret"
        );
        assert!(
            !serde_json::to_string(&created)
                .unwrap()
                .contains("env-secret")
        );
        let renamed = service
            .write_profile(WriteConfigProfileRequest::Rename {
                source: reference(&created),
                name: "Renamed".into(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            service
                .checked_profile(&reference(&renamed))
                .unwrap()
                .setting_overrides,
            saved.setting_overrides
        );
        assert!(matches!(
            service.write_profile(WriteConfigProfileRequest::Delete {
                source: reference(&created)
            }),
            Err(AgentSettingError::StaleRevision)
        ));
        let duplicate = service
            .write_profile(WriteConfigProfileRequest::Duplicate {
                source: reference(&renamed),
                name: "Duplicate".into(),
            })
            .unwrap()
            .unwrap();
        assert_ne!(duplicate.id, renamed.id);
        assert_eq!(
            service
                .checked_profile(&reference(&duplicate))
                .unwrap()
                .environment,
            saved.environment
        );
        let request = CopyProfileWriteRequest {
            source: reference(&renamed),
            target_provider: AgentSettingsProvider::ClaudeCode,
            target_executor_profile: ExecutorProfileId::new(BaseCodingAgent::ClaudeCode),
            target_name: "Copy".into(),
        };
        let preview = service.preview_profile_copy_view(request.clone()).unwrap();
        assert!(
            !serde_json::to_string(&preview)
                .unwrap()
                .contains("env-secret")
        );
        let copied = service
            .write_profile(WriteConfigProfileRequest::Copy { request })
            .unwrap()
            .unwrap();
        assert_eq!(
            service
                .checked_profile(&reference(&copied))
                .unwrap()
                .environment,
            saved.environment
        );
    }

    #[test]
    fn profile_apply_requires_same_profile_and_native_revisions_and_returns_safe_snapshot() {
        let (_root, service, file) = fixture();
        let profile = capture(&service);
        let snapshot = snapshot(&service);
        let request = ProfileApplyRequest {
            reference: reference(&profile),
            project_path: None,
            scope: SettingScope::User,
            expected_file_revisions: snapshot
                .native_files
                .iter()
                .map(|f| (f.id.clone(), f.revision.clone().unwrap()))
                .collect(),
        };
        assert!(
            !serde_json::to_string(&service.preview_profile_apply_view(&request).unwrap())
                .unwrap()
                .contains("env-secret")
        );
        let applied = service
            .apply_profile_view(ConfirmProfileApplyRequest {
                preview: request.clone(),
                confirmed: true,
            })
            .unwrap();
        assert!(
            !serde_json::to_string(&applied)
                .unwrap()
                .contains("env-secret")
        );
        fs::write(&file, "model='external'\n").unwrap();
        let before = fs::read(&file).unwrap();
        assert!(matches!(
            service.apply_profile_view(ConfirmProfileApplyRequest {
                preview: request.clone(),
                confirmed: true
            }),
            Err(AgentSettingError::StaleRevision)
        ));
        assert_eq!(fs::read(&file).unwrap(), before);
        service
            .write_profile(WriteConfigProfileRequest::Rename {
                source: reference(&profile),
                name: "Changed".into(),
            })
            .unwrap();
        assert!(matches!(
            service.preview_profile_apply_view(&request),
            Err(AgentSettingError::StaleRevision)
        ));
    }

    #[test]
    fn malformed_native_file_diagnostics_do_not_echo_input_and_advanced_read_remains_available() {
        let (_root, service, file) = fixture();
        fs::write(file, "secret = 'malformed-private-line\n").unwrap();
        let original = snapshot(&service);
        let view: AgentSettingsInventoryView = service
            .discover(Some(AgentSettingsProvider::Codex), None)
            .into();
        assert!(
            !serde_json::to_string(&view)
                .unwrap()
                .contains("malformed-private-line")
        );
        let file = original.native_files.iter().find(|f| f.exists).unwrap();
        assert!(
            service
                .read_native_settings(ReadNativeSettingsRequest {
                    provider: original.provider,
                    project_path: None,
                    file_id: file.id.clone(),
                    expected_revision: file.revision.clone().unwrap(),
                    confirmed_sensitive_read: true
                })
                .unwrap()
                .raw_content
                .unwrap()
                .contains("malformed-private-line")
        );
        assert!(
            !serde_json::to_string(&public_error(
                AgentSettingError::InvalidConfiguration("secret-line".into()),
                None
            ))
            .unwrap()
            .contains("secret-line")
        );
    }
}
