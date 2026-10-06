use db::models::{
    resource_coordination::{
        ResourceClaim, ResourceOperation, ResourceOperationSpec, ResourceSnapshot,
    },
    session::Session,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, schemars, tool,
    tool_router,
};
use serde::Deserialize;
use uuid::Uuid;

use super::McpServer;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct Claim {
    resource_id: Uuid,
    expected_revision: i64,
    resulting_state: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RunResourceOperation {
    /// Stable UUID for retries; use a new ID after deliberately revising a plan.
    request_id: Uuid,
    /// Your current LVK session, supplied in the host's runtime instructions.
    session_id: Uuid,
    purpose: String,
    claims: Vec<Claim>,
    /// Full critical section and cleanup, with no detached/background activity.
    script: String,
    /// Check all resources are idle and in the declared resulting state.
    verification_script: String,
    /// Relative to the workspace root (use the repository name for a worktree).
    working_dir: String,
    /// Total command + verification deadline, 1–3600 seconds.
    timeout_seconds: u32,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OperationId {
    operation_id: Uuid,
}

impl McpServer {
    async fn owned_resource_operation(
        &self,
        id: Uuid,
    ) -> Result<ResourceOperation, super::ToolError> {
        let op: ResourceOperation = self
            .send_json(
                self.client
                    .get(self.url(&format!("/api/resource-coordination/operations/{id}"))),
            )
            .await?;
        self.resource_workspace_scope(op.workspace_id)?;
        Ok(op)
    }
    fn resource_workspace_scope(&self, id: Uuid) -> Result<(), super::ToolError> {
        if self
            .scoped_workspace_id()
            .is_some_and(|current| current != id)
        {
            return Err(super::ToolError::message(
                "A resource operation must belong to your current workspace",
            ));
        }
        self.scope_allows_workspace(id)
    }
}

#[tool_router(router=resource_tools_router,vis="pub")]
impl McpServer {
    #[tool(
        description = "List shared resources, current state revisions, exclusive owners and pending purposes. Use before any device/DB/deployment access. One physical resource must use one canonical key. Scripts and private conversations are excluded."
    )]
    async fn list_shared_resources(&self) -> Result<CallToolResult, ErrorData> {
        match self
            .send_json::<ResourceSnapshot>(
                self.client
                    .get(self.url("/api/resource-coordination/snapshot")),
            )
            .await
        {
            Ok(view) => Self::success(&view),
            Err(e) => Ok(Self::tool_error(e)),
        }
    }
    #[tool(
        description = "Queue a durable command under an atomic exclusive resource bundle. LVK runs it when all claims are available and revisions still match. Include the complete critical section, cleanup and a real idle/state verification. Do not detach work, hold resources across model turns, bypass owners or blindly update expected revisions. A queued request is not task completion. Retry with the same request_id and identical contents."
    )]
    async fn run_resource_operation(
        &self,
        Parameters(r): Parameters<RunResourceOperation>,
    ) -> Result<CallToolResult, ErrorData> {
        let session: Session = match self
            .send_json(
                self.client
                    .get(self.url(&format!("/api/sessions/{}", r.session_id))),
            )
            .await
        {
            Ok(s) => s,
            Err(e) => return Ok(Self::tool_error(e)),
        };
        if let Err(e) = self.resource_workspace_scope(session.workspace_id) {
            return Ok(Self::tool_error(e));
        }
        let spec = ResourceOperationSpec {
            request_id: r.request_id,
            session_id: r.session_id,
            purpose: r.purpose,
            claims: r
                .claims
                .into_iter()
                .map(|c| ResourceClaim {
                    resource_id: c.resource_id,
                    expected_revision: c.expected_revision,
                    resulting_state: c.resulting_state,
                })
                .collect(),
            script: r.script,
            verification_script: r.verification_script,
            working_dir: r.working_dir,
            timeout_seconds: r.timeout_seconds,
        };
        match self
            .send_json::<ResourceOperation>(
                self.client
                    .post(self.url("/api/resource-coordination/operations"))
                    .json(&spec),
            )
            .await
        {
            Ok(op) => Self::success(&op),
            Err(e) => Ok(Self::tool_error(e)),
        }
    }
    #[tool(
        description = "Wait up to 25 seconds for your managed resource operation. Waiting consumes no model tokens. Inspect status and process_id for logs. blocked requires a new deliberate plan; recovery_required needs operator confirmation. Never interpret queued/running as completion or start a duplicate command."
    )]
    async fn wait_resource_operation(
        &self,
        Parameters(r): Parameters<OperationId>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Err(e) = self.owned_resource_operation(r.operation_id).await {
            return Ok(Self::tool_error(e));
        }
        match self
            .send_json::<ResourceOperation>(self.client.get(self.url(&format!(
                "/api/resource-coordination/operations/{}?wait_seconds=25",
                r.operation_id
            ))))
            .await
        {
            Ok(op) => Self::success(&op),
            Err(e) => Ok(Self::tool_error(e)),
        }
    }
    #[tool(
        description = "Cancel your queued or running operation. Cancellation of active work does not release resources: LVK confirms process termination and requires recovery of uncertain external state. No force unlock is available to agents."
    )]
    async fn cancel_resource_operation(
        &self,
        Parameters(r): Parameters<OperationId>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Err(e) = self.owned_resource_operation(r.operation_id).await {
            return Ok(Self::tool_error(e));
        }
        match self
            .send_json::<ResourceOperation>(self.client.post(self.url(&format!(
                "/api/resource-coordination/operations/{}/cancel",
                r.operation_id
            ))))
            .await
        {
            Ok(op) => Self::success(&op),
            Err(e) => Ok(Self::tool_error(e)),
        }
    }
}
