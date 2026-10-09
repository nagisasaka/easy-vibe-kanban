//! Durable delivery only. The resource coordinator remains the sole lock owner.
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection, SqlitePool, types::Json};
use ts_rs::TS;
use uuid::Uuid;

use super::resource_coordination::ResourceOperationSpec;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(deny_unknown_fields)]
pub struct RunnerTarget {
    pub runner_id: Uuid,
    pub source_id: Uuid,
    /// Keep the bundle until an explicit finish, permitting observation/steps.
    pub interactive: bool,
    pub desktop: bool,
    pub cleanup_script: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Runner {
    pub id: Uuid,
    pub name: String,
    pub execution_resource_id: Uuid,
    pub capabilities: Json<serde_json::Value>,
    pub last_seen: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    pub id: Uuid,
    pub kind: String,
    pub script: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Command {
    pub sequence: i64,
    pub id: Uuid,
    pub operation_id: Uuid,
    pub kind: String,
    pub script: String,
    pub status: String,
    pub result: Option<Json<serde_json::Value>>,
}

pub async fn validate_target(
    conn: &mut SqliteConnection,
    spec: &ResourceOperationSpec,
    workspace: Uuid,
) -> anyhow::Result<()> {
    let Some(target) = &spec.runner else {
        return Ok(());
    };
    ensure!(
        !target.cleanup_script.trim().is_empty() && target.cleanup_script.len() <= 65536,
        "Provide a bounded cleanup script"
    );
    let runner: Runner = sqlx::query_as("SELECT id,name,execution_resource_id,capabilities,last_seen,disabled FROM bridge_runners WHERE id=?")
        .bind(target.runner_id).fetch_optional(&mut *conn).await?.context("Unknown runner")?;
    ensure!(!runner.disabled, "Runner disabled");
    ensure!(
        runner.capabilities["protocol"] == 1,
        "Runner must connect using bridge protocol 1 before submission"
    );
    ensure!(
        spec.claims
            .iter()
            .any(|c| c.resource_id == runner.execution_resource_id),
        "Claim the runner's registered execution slot resource"
    );
    let source_workspace: Uuid =
        sqlx::query_scalar("SELECT workspace_id FROM bridge_sources WHERE id=?")
            .bind(target.source_id)
            .fetch_optional(&mut *conn)
            .await?
            .context("Unknown source snapshot")?;
    ensure!(
        source_workspace == workspace,
        "Source belongs to a different workspace"
    );
    if target.desktop {
        ensure!(
            runner.capabilities["desktop"] == true,
            "Runner has not reported an interactive user desktop"
        );
    }
    Ok(())
}

/// Stable command UUIDs are immutable. Only one outstanding command, and finish
/// permanently closes admission. All checks and insertion share one transaction.
pub async fn enqueue(
    pool: &SqlitePool,
    operation: Uuid,
    request: CommandRequest,
) -> anyhow::Result<Command> {
    ensure!(
        matches!(request.kind.as_str(), "step" | "finish"),
        "Expected step or finish"
    );
    ensure!(request.script.len() <= 65536, "Script too large");
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if let Some(old) = sqlx::query_as::<_, Command>("SELECT * FROM bridge_commands WHERE id=?")
        .bind(request.id)
        .fetch_optional(&mut *tx)
        .await?
    {
        ensure!(
            old.operation_id == operation
                && old.kind == request.kind
                && old.script == request.script,
            "Command ID content conflict"
        );
        return Ok(old);
    }
    let op: super::resource_coordination::ResourceOperation =
        sqlx::query_as("SELECT * FROM resource_operations WHERE id=?")
            .bind(operation)
            .fetch_one(&mut *tx)
            .await?;
    ensure!(
        op.status == "running" && !op.cancel_requested,
        "Operation is not accepting commands"
    );
    ensure!(
        op.spec.runner.as_ref().is_some_and(|r| r.interactive),
        "Operation is not interactive"
    );
    let busy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bridge_commands WHERE operation_id=? AND (status<>'done' OR kind='finish')) OR EXISTS(SELECT 1 FROM bridge_settlements WHERE operation_id=?)").bind(operation).bind(operation).fetch_one(&mut *tx).await?;
    ensure!(
        !busy,
        "Wait for the outstanding command; finish closes the operation"
    );
    ensure!(
        request.kind != "finish" || request.script.is_empty(),
        "Finish uses the immutable cleanup and verification scripts"
    );
    sqlx::query("INSERT INTO bridge_commands(id,operation_id,kind,script) VALUES(?,?,?,?)")
        .bind(request.id)
        .bind(operation)
        .bind(request.kind)
        .bind(request.script)
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query_as("SELECT * FROM bridge_commands WHERE id=?")
        .bind(request.id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(row)
}
