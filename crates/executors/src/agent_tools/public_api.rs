//! Safe management DTOs. Full native definitions stay inside the provider
//! adapter, except for an explicit revision-checked sensitive read.
use super::*;
use crate::config_write::SensitiveWrite;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct McpServerWriteDefinition {
    pub transport: McpTransport,
    pub command: SensitiveWrite<Option<String>>,
    pub args: SensitiveWrite<Vec<String>>,
    pub cwd: SensitiveWrite<Option<String>>,
    pub url: SensitiveWrite<Option<String>>,
    pub env: SensitiveWrite<BTreeMap<String, String>>,
    pub headers: SensitiveWrite<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SkillWriteDefinition {
    Preserve,
    Replace { value: SkillDefinition },
    ReplaceContract { value: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentToolWriteDefinition {
    McpServer(McpServerWriteDefinition),
    Skill(SkillWriteDefinition),
}

impl AgentToolWriteDefinition {
    fn resolve(
        self,
        current: Option<&AgentToolDefinition>,
    ) -> Result<AgentToolDefinition, AgentToolError> {
        let invalid = |message: &str| AgentToolError::InvalidRequest(message.into());
        Ok(match self {
            Self::McpServer(write) => {
                let current = current.and_then(|definition| match definition {
                    AgentToolDefinition::McpServer(value) => Some(value),
                    _ => None,
                });
                AgentToolDefinition::McpServer(McpServerDefinition {
                    transport: write.transport,
                    command: write
                        .command
                        .resolve(current.map(|v| &v.command))
                        .map_err(invalid)?,
                    args: write
                        .args
                        .resolve(current.map(|v| &v.args))
                        .map_err(invalid)?,
                    cwd: write
                        .cwd
                        .resolve(current.map(|v| &v.cwd))
                        .map_err(invalid)?,
                    url: write
                        .url
                        .resolve(current.map(|v| &v.url))
                        .map_err(invalid)?,
                    env: write
                        .env
                        .resolve(current.map(|v| &v.env))
                        .map_err(invalid)?,
                    headers: write
                        .headers
                        .resolve(current.map(|v| &v.headers))
                        .map_err(invalid)?,
                    source_metadata: current
                        .map(|v| v.source_metadata.clone())
                        .unwrap_or(Value::Null),
                })
            }
            Self::Skill(write) => {
                let current = current.and_then(|definition| match definition {
                    AgentToolDefinition::Skill(value) => Some(value),
                    _ => None,
                });
                AgentToolDefinition::Skill(match write {
                    SkillWriteDefinition::Preserve => current
                        .cloned()
                        .ok_or_else(|| invalid("Preserve requires an existing Skill"))?,
                    SkillWriteDefinition::Replace { value } => value,
                    SkillWriteDefinition::ReplaceContract { value } => {
                        let mut definition = current.cloned().unwrap_or(SkillDefinition {
                            description: None,
                            files: Vec::new(),
                        });
                        definition.files.retain(|file| file.path != "SKILL.md");
                        definition.files.insert(
                            0,
                            SkillFile {
                                path: "SKILL.md".into(),
                                content_base64: BASE64.encode(value),
                            },
                        );
                        definition
                    }
                })
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum AgentToolSummary {
    McpServer {
        transport: McpTransport,
        command_configured: bool,
        args_count: usize,
        url_configured: bool,
        env_count: usize,
        header_count: usize,
        has_provider_extensions: bool,
    },
    Skill {
        file_count: usize,
        has_assets: bool,
    },
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct AgentToolView {
    pub provider: AgentToolProvider,
    pub scope: AgentToolScope,
    pub kind: AgentToolKind,
    pub name: String,
    pub native_path: String,
    pub state: AgentToolState,
    pub capabilities: AgentToolCapabilities,
    pub revision: String,
    pub definition: AgentToolSummary,
    pub error: Option<String>,
}

impl From<AgentTool> for AgentToolView {
    fn from(item: AgentTool) -> Self {
        let definition = match item.definition {
            AgentToolDefinition::McpServer(value) => AgentToolSummary::McpServer {
                transport: value.transport,
                command_configured: value.command.is_some(),
                args_count: value.args.len(),
                url_configured: value.url.is_some(),
                env_count: value.env.len(),
                header_count: value.headers.len(),
                has_provider_extensions: !value.source_metadata.is_null(),
            },
            AgentToolDefinition::Skill(value) => AgentToolSummary::Skill {
                file_count: value.files.len(),
                has_assets: value.files.iter().any(|file| file.path != "SKILL.md"),
            },
        };
        Self {
            provider: item.provider,
            scope: item.scope,
            kind: item.kind,
            name: item.name,
            native_path: item.native_path,
            state: item.state,
            capabilities: item.capabilities,
            revision: item.revision,
            definition,
            error: item.error.map(|_| {
                "Invalid native tool configuration; inspect the native file explicitly".into()
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct AgentToolProviderInventoryView {
    pub provider: AgentToolProvider,
    pub installed: bool,
    pub items: Vec<AgentToolView>,
    pub limitations: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct AgentToolInventoryView {
    pub providers: Vec<AgentToolProviderInventoryView>,
    pub errors: Vec<AgentToolProviderError>,
}

impl From<AgentToolInventory> for AgentToolInventoryView {
    fn from(inventory: AgentToolInventory) -> Self {
        Self {
            providers: inventory.providers.into_iter().map(|provider| AgentToolProviderInventoryView {
                provider: provider.provider, installed: provider.installed,
                items: provider.items.into_iter().map(Into::into).collect(),
                limitations: provider.limitations,
                errors: provider.errors.into_iter().map(|_| "Could not read a native tool configuration. Raw parse diagnostics are withheld.".into()).collect(),
            }).collect(),
            errors: inventory.errors.into_iter().map(|error| AgentToolProviderError {
                provider: error.provider, message: "Could not discover provider tools. Inspect native configuration.".into(),
            }).collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct CreateAgentToolWriteRequest {
    pub target: AgentToolLocator,
    pub definition: AgentToolWriteDefinition,
    #[serde(default)]
    pub replace: bool,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct UpdateAgentToolWriteRequest {
    pub target: AgentToolLocator,
    pub definition: AgentToolWriteDefinition,
    pub expected_revision: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct ReadAgentToolDefinitionRequest {
    pub target: AgentToolLocator,
    pub expected_revision: String,
    pub confirmed_sensitive_read: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct CopyAgentToolView {
    pub item: AgentToolView,
    pub warnings: Vec<String>,
}

impl From<CopyAgentToolResponse> for CopyAgentToolView {
    fn from(response: CopyAgentToolResponse) -> Self {
        Self {
            item: response.item.into(),
            warnings: response.warnings,
        }
    }
}

impl AgentToolService {
    pub fn create_from_write(
        &self,
        request: CreateAgentToolWriteRequest,
    ) -> Result<AgentToolView, AgentToolError> {
        let current = match self.get(&request.target) {
            Ok(item) => Some(item),
            Err(AgentToolError::NotFound(_)) => None,
            Err(error) => return Err(error),
        };
        if let Some(current) = &current {
            if !request.replace {
                return Err(AgentToolError::Collision(request.target.name));
            }
            ensure_revision(
                &current.revision,
                request.expected_revision.as_deref().ok_or_else(|| {
                    AgentToolError::InvalidRequest("Replacement requires a revision".into())
                })?,
            )?;
        }
        let definition = request
            .definition
            .resolve(current.as_ref().map(|item| &item.definition))?;
        self.create(CreateAgentToolRequest {
            target: request.target,
            definition,
            replace: request.replace,
            expected_revision: request.expected_revision,
        })
        .map(Into::into)
    }

    pub fn update_from_write(
        &self,
        request: UpdateAgentToolWriteRequest,
    ) -> Result<AgentToolView, AgentToolError> {
        let current = self.get(&request.target)?;
        ensure_revision(&current.revision, &request.expected_revision)?;
        let definition = request.definition.resolve(Some(&current.definition))?;
        self.update(UpdateAgentToolRequest {
            target: request.target,
            definition,
            expected_revision: request.expected_revision,
        })
        .map(Into::into)
    }

    pub fn read_definition(
        &self,
        request: ReadAgentToolDefinitionRequest,
    ) -> Result<AgentToolDefinition, AgentToolError> {
        if !request.confirmed_sensitive_read {
            return Err(AgentToolError::InvalidRequest(
                "Reading native contents may expose credentials; explicit confirmation is required"
                    .into(),
            ));
        }
        let item = self.get(&request.target)?;
        ensure_revision(&item.revision, &request.expected_revision)?;
        Ok(item.definition)
    }
}

pub fn public_tool_error(
    error: AgentToolError,
    provider: Option<AgentToolProvider>,
    name: Option<String>,
) -> AgentToolOperationError {
    let mut error = error.operation_error(provider, name);
    // Native parse diagnostics may embed an offending secret-bearing line.
    error.message = match error.code {
        AgentToolErrorCode::StaleRevision => "Native content changed; refresh before editing",
        AgentToolErrorCode::Collision => {
            "An installation already exists; confirm replacement with its current revision"
        }
        AgentToolErrorCode::NotFound => "Tool installation was not found",
        AgentToolErrorCode::InvalidRequest => {
            "Invalid tool write intent or missing explicit confirmation"
        }
        AgentToolErrorCode::Unsupported => {
            "This operation is not supported for the selected installation"
        }
        AgentToolErrorCode::UnsafePath => "Unsafe tool path was rejected",
        AgentToolErrorCode::InvalidConfiguration => {
            "Invalid native configuration; inspect the native file explicitly"
        }
        AgentToolErrorCode::Io => "Native configuration I/O failed",
        AgentToolErrorCode::VerificationFailed => "The native write could not be verified",
    }
    .into();
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original() -> AgentToolDefinition {
        AgentToolDefinition::McpServer(McpServerDefinition {
            transport: McpTransport::Http,
            command: Some("command-secret".into()),
            args: vec!["arg-secret".into()],
            cwd: None,
            url: Some("https://user:secret@example.test/path?token=secret".into()),
            env: BTreeMap::from([("TOKEN".into(), "env-secret".into())]),
            headers: BTreeMap::from([("Authorization".into(), "header-secret".into())]),
            source_metadata: serde_json::json!({"unknown":"extension-secret"}),
        })
    }

    fn preserve() -> AgentToolWriteDefinition {
        AgentToolWriteDefinition::McpServer(McpServerWriteDefinition {
            transport: McpTransport::Http,
            command: SensitiveWrite::Preserve,
            args: SensitiveWrite::Preserve,
            cwd: SensitiveWrite::Preserve,
            url: SensitiveWrite::Preserve,
            env: SensitiveWrite::Preserve,
            headers: SensitiveWrite::Preserve,
        })
    }

    #[test]
    fn write_intent_never_uses_redacted_placeholders_or_discards_unknown_native_fields() {
        let current = original();
        assert_eq!(preserve().resolve(Some(&current)).unwrap(), current);
        assert!(preserve().resolve(None).is_err());
        let AgentToolWriteDefinition::McpServer(mut write) = preserve() else {
            panic!()
        };
        write.env = SensitiveWrite::Clear;
        write.headers = SensitiveWrite::Replace {
            value: BTreeMap::from([("New".into(), "new-secret".into())]),
        };
        let AgentToolDefinition::McpServer(next) = AgentToolWriteDefinition::McpServer(write)
            .resolve(Some(&current))
            .unwrap()
        else {
            panic!()
        };
        assert!(next.env.is_empty());
        assert_eq!(next.headers["New"], "new-secret");
        assert_eq!(next.source_metadata["unknown"], "extension-secret");
    }

    #[test]
    fn discovery_and_copy_summary_do_not_serialize_native_values_or_raw_errors() {
        let item = AgentTool {
            provider: AgentToolProvider::Codex,
            scope: AgentToolScope::User,
            kind: AgentToolKind::McpServer,
            name: "test".into(),
            native_path: "/config".into(),
            state: AgentToolState::Enabled,
            capabilities: Default::default(),
            revision: "hash".into(),
            definition: original(),
            error: Some("parse-secret".into()),
        };
        let view = AgentToolView::from(item.clone());
        let serialized = serde_json::to_string(&view).unwrap();
        assert!(!serialized.contains("secret"));
        let copy = CopyAgentToolView::from(CopyAgentToolResponse {
            item,
            source_metadata: serde_json::json!({"auth":"copy-secret"}),
            warnings: Vec::new(),
        });
        assert!(!serde_json::to_string(&copy).unwrap().contains("secret"));
        let error = public_tool_error(
            AgentToolError::InvalidConfiguration("parse-secret".into()),
            None,
            None,
        );
        assert!(!error.message.contains("parse-secret"));
    }

    #[test]
    fn contract_edit_preserves_unread_skill_assets() {
        let current = AgentToolDefinition::Skill(SkillDefinition {
            description: None,
            files: vec![
                SkillFile {
                    path: "asset.bin".into(),
                    content_base64: "asset-secret".into(),
                },
                SkillFile {
                    path: "SKILL.md".into(),
                    content_base64: "old".into(),
                },
            ],
        });
        let AgentToolDefinition::Skill(next) =
            AgentToolWriteDefinition::Skill(SkillWriteDefinition::ReplaceContract {
                value: "new".into(),
            })
            .resolve(Some(&current))
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(next.files.len(), 2);
        assert_eq!(next.files[0].content_base64, BASE64.encode("new"));
        assert_eq!(next.files[1].content_base64, "asset-secret");
    }

    #[test]
    fn native_tool_write_path_preserves_hidden_fields_and_rejects_stale_edits() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let path = home.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "model='keep'\n[mcp_servers.example]\ncommand='run'\nunknown='extension-secret'\n[mcp_servers.example.env]\nTOKEN='env-secret'\n").unwrap();
        let service = AgentToolService::new(home, root.path().join("disabled"));
        let target = AgentToolLocator {
            provider: AgentToolProvider::Codex,
            scope: AgentToolScope::User,
            kind: AgentToolKind::McpServer,
            name: "example".into(),
            native_path: None,
            project_path: None,
        };
        let original = service.get(&target).unwrap();
        let inventory: AgentToolInventoryView = service.discover(None).into();
        assert!(
            !serde_json::to_string(&inventory)
                .unwrap()
                .contains("env-secret")
        );
        assert!(
            !serde_json::to_string(&inventory)
                .unwrap()
                .contains("extension-secret")
        );
        let mut read = ReadAgentToolDefinitionRequest {
            target: target.clone(),
            expected_revision: original.revision.clone(),
            confirmed_sensitive_read: false,
        };
        assert!(service.read_definition(read.clone()).is_err());
        read.confirmed_sensitive_read = true;
        assert!(
            serde_json::to_string(&service.read_definition(read.clone()).unwrap())
                .unwrap()
                .contains("env-secret")
        );
        let AgentToolWriteDefinition::McpServer(mut write) = preserve() else {
            panic!()
        };
        write.transport = McpTransport::Stdio;
        write.command = SensitiveWrite::Replace {
            value: Some("new-command".into()),
        };
        let update = UpdateAgentToolWriteRequest {
            target: target.clone(),
            expected_revision: original.revision,
            definition: AgentToolWriteDefinition::McpServer(write.clone()),
        };
        let edited = service.update_from_write(update.clone()).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("env-secret")
                && content.contains("extension-secret")
                && content.contains("keep")
        );
        assert!(
            !serde_json::to_string(&edited)
                .unwrap()
                .contains("env-secret")
        );
        assert!(matches!(
            service.update_from_write(update),
            Err(AgentToolError::StaleRevision)
        ));
        write.env = SensitiveWrite::Clear;
        service
            .update_from_write(UpdateAgentToolWriteRequest {
                target: target.clone(),
                expected_revision: edited.revision,
                definition: AgentToolWriteDefinition::McpServer(write),
            })
            .unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("env-secret"));
        assert!(matches!(
            service.read_definition(read),
            Err(AgentToolError::StaleRevision)
        ));
    }
}
