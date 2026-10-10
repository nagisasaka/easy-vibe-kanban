//! Pull transport for explicitly enrolled execution runners. No lease unlocks.
mod source;

use anyhow::{Context, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use db::models::{
    execution_bridge::{self as model, Command, CommandRequest, Runner},
    resource_coordination::{ResourceHolder, ResourceOperation},
};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::types::Json as SqlJson;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

fn error(e: impl std::fmt::Display) -> ApiError {
    ApiError::BadRequest(e.to_string())
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    name: String,
    execution_resource_id: Uuid,
}
async fn enroll(
    State(d): State<DeploymentImpl>,
    Json(r): Json<Enrollment>,
) -> Result<Json<ApiResponse<Value>>, ApiError> {
    if r.name.trim().is_empty() || r.name.len() > 200 {
        return Err(error("Invalid runner name"));
    }
    let id = Uuid::new_v4();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO bridge_runners(id,name,token_hash,execution_resource_id) VALUES(?,?,?,?)",
    )
    .bind(id)
    .bind(r.name)
    .bind(hash(&token))
    .bind(r.execution_resource_id)
    .execute(&d.db().pool)
    .await?;
    Ok(Json(ApiResponse::success(json!({"id":id,"token":token}))))
}
async fn rotate_token(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Value>>, ApiError> {
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let changed = sqlx::query("UPDATE bridge_runners SET token_hash=? WHERE id=?")
        .bind(hash(&token))
        .bind(id)
        .execute(&d.db().pool)
        .await?
        .rows_affected();
    if changed != 1 {
        return Err(error("Unknown runner"));
    }
    Ok(Json(ApiResponse::success(json!({"id":id,"token":token}))))
}

async fn runners(
    State(d): State<DeploymentImpl>,
) -> Result<Json<ApiResponse<Vec<Runner>>>, ApiError> {
    let rows = sqlx::query_as("SELECT id,name,execution_resource_id,capabilities,last_seen,disabled FROM bridge_runners ORDER BY name").fetch_all(&d.db().pool).await?;
    Ok(Json(ApiResponse::success(rows)))
}
async fn disable(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    // Prevent new dispatch. Existing authenticated runner must still drain/cancel.
    sqlx::query("UPDATE bridge_runners SET disabled=1 WHERE id=?")
        .bind(id)
        .execute(&d.db().pool)
        .await?;
    Ok(Json(ApiResponse::success(())))
}
async fn authenticate(
    pool: &sqlx::SqlitePool,
    id: Uuid,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let token = headers
        .get("x-lvk-runner-token")
        .and_then(|h| h.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let installation = installation(headers)?;
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bridge_runners WHERE id=? AND token_hash=? AND (installation_id IS NULL OR installation_id=?))",
    )
    .bind(id)
    .bind(hash(token))
    .bind(installation)
    .fetch_one(pool)
    .await?;
    if !valid {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

fn installation(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    headers
        .get("x-lvk-runner-installation")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or(ApiError::Unauthorized)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Poll {
    capabilities: Value,
    #[serde(default)]
    known_operations: Vec<Uuid>,
}
#[derive(Serialize)]
struct Delivery {
    operation: ResourceOperation,
    source: Value,
    fences: Vec<ResourceHolder>,
    commands: Vec<Command>,
}
async fn poll(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(p): Json<Poll>,
) -> Result<Json<ApiResponse<Vec<Delivery>>>, ApiError> {
    authenticate(&d.db().pool, id, &headers).await?;
    if serde_json::to_vec(&p.capabilities).map_err(error)?.len() > 8192
        || p.known_operations.len() > 1000
    {
        return Err(error("Capabilities too large"));
    }
    let mut tx = d.db().pool.begin_with("BEGIN IMMEDIATE").await?;
    let installation = installation(&headers)?;
    let changed = sqlx::query(
        "UPDATE bridge_runners SET installation_id=?,capabilities=?,last_seen=datetime('now','subsec') WHERE id=? AND (installation_id IS NULL OR installation_id=?)",
    )
    .bind(installation)
    .bind(SqlJson(&p.capabilities))
    .bind(id)
    .bind(installation)
    .execute(&mut *tx)
    .await?.rows_affected();
    if changed != 1 {
        return Err(ApiError::Unauthorized);
    }
    let ops: Vec<ResourceOperation> = sqlx::query_as("SELECT * FROM resource_operations WHERE json_extract(spec,'$.runner.runner_id')=? AND status IN ('running','recovery_required') AND NOT EXISTS(SELECT 1 FROM bridge_settlements s WHERE s.operation_id=resource_operations.id) ORDER BY sequence")
        .bind(id.to_string()).fetch_all(&mut *tx).await?;
    let mut deliveries = vec![];
    for op in ops {
        let target = op
            .spec
            .runner
            .as_ref()
            .ok_or_else(|| error("Missing runner target"))?;
        let source: SqlJson<Value> = sqlx::query_scalar(if p.known_operations.contains(&op.id) {
            "SELECT json_object('digest',digest) FROM bridge_sources WHERE id=?"
        } else {
            "SELECT manifest FROM bridge_sources WHERE id=?"
        })
        .bind(target.source_id)
        .fetch_one(&mut *tx)
        .await?;
        let fences = sqlx::query_as(
            "SELECT * FROM resource_holders WHERE operation_id=? ORDER BY resource_id",
        )
        .bind(op.id)
        .fetch_all(&mut *tx)
        .await?;
        // Sent work is replayed with the SAME identity, never re-issued as new work.
        sqlx::query(
            "UPDATE bridge_commands SET status='sent' WHERE operation_id=? AND status='pending'",
        )
        .bind(op.id)
        .execute(&mut *tx)
        .await?;
        let commands = sqlx::query_as("SELECT * FROM bridge_commands WHERE operation_id=? AND status='sent' ORDER BY sequence").bind(op.id).fetch_all(&mut *tx).await?;
        deliveries.push(Delivery {
            operation: op,
            source: source.0,
            fences,
            commands,
        });
    }
    tx.commit().await?;
    Ok(Json(ApiResponse::success(deliveries)))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    runtime_id: Uuid,
    command_id: Uuid,
    success: bool,
    settled: bool,
    output: String,
    artifacts: Vec<source::SourceFile>,
}
async fn report(
    State(d): State<DeploymentImpl>,
    Path((runner, operation)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(r): Json<Report>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    authenticate(&d.db().pool, runner, &headers).await?;
    save_report(&d.db().pool, runner, operation, r).await?;
    Ok(Json(ApiResponse::success(())))
}

async fn save_report(
    pool: &sqlx::SqlitePool,
    runner: Uuid,
    operation: Uuid,
    r: Report,
) -> Result<(), ApiError> {
    if r.output.len() > 1_048_576 {
        return Err(error("Log exceeds 1 MiB"));
    }
    source::validate_files(&r.artifacts).map_err(error)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let op: ResourceOperation = sqlx::query_as("SELECT * FROM resource_operations WHERE id=?")
        .bind(operation)
        .fetch_one(&mut *tx)
        .await?;
    check_dispatch(&op, runner, r.runtime_id).map_err(error)?;
    let command: Command =
        sqlx::query_as("SELECT * FROM bridge_commands WHERE operation_id=? AND id=?")
            .bind(operation)
            .bind(r.command_id)
            .fetch_one(&mut *tx)
            .await?;
    let value = serde_json::to_value(&r).map_err(error)?;
    if let Some(old) = command.result {
        if old.0 != value {
            return Err(error("Conflicting completion retry"));
        }
    } else {
        if command.status != "sent" {
            return Err(error("Command was not delivered"));
        }
        sqlx::query("UPDATE bridge_commands SET result=?,status='done' WHERE id=?")
            .bind(SqlJson(value))
            .bind(r.command_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
fn check_dispatch(op: &ResourceOperation, runner: Uuid, runtime: Uuid) -> anyhow::Result<()> {
    ensure!(
        op.spec
            .runner
            .as_ref()
            .is_some_and(|r| r.runner_id == runner)
            && op.runtime_id == Some(runtime),
        "Stale dispatch or wrong runner"
    );
    ensure!(
        matches!(
            op.status.as_str(),
            "running" | "recovery_required" | "succeeded"
        ),
        "Dispatch is closed"
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settlement {
    runtime_id: Uuid,
    evidence: String,
}
async fn settled(
    State(d): State<DeploymentImpl>,
    Path((runner, operation)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(r): Json<Settlement>,
) -> Result<Json<ApiResponse<()>>, ApiError> {
    authenticate(&d.db().pool, runner, &headers).await?;
    if r.evidence.is_empty() || r.evidence.len() > 4000 {
        return Err(error("Settlement evidence required"));
    }
    let mut tx = d.db().pool.begin_with("BEGIN IMMEDIATE").await?;
    let op: ResourceOperation = sqlx::query_as("SELECT * FROM resource_operations WHERE id=?")
        .bind(operation)
        .fetch_one(&mut *tx)
        .await?;
    check_dispatch(&op, runner, r.runtime_id).map_err(error)?;
    let outstanding: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bridge_commands WHERE operation_id=? AND status<>'done')",
    )
    .bind(operation)
    .fetch_one(&mut *tx)
    .await?;
    if outstanding {
        return Err(error("Deliver all command receipts before settlement"));
    }
    sqlx::query("INSERT INTO bridge_settlements(operation_id,runtime_id,evidence) VALUES(?,?,?) ON CONFLICT(operation_id) DO NOTHING").bind(operation).bind(r.runtime_id).bind(r.evidence).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(ApiResponse::success(())))
}
async fn enqueue(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
    Json(r): Json<CommandRequest>,
) -> Result<Json<ApiResponse<Command>>, ApiError> {
    Ok(Json(ApiResponse::success(
        model::enqueue(&d.db().pool, id, r).await.map_err(error)?,
    )))
}
async fn commands(
    State(d): State<DeploymentImpl>,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiResponse<Vec<Command>>>, ApiError> {
    Ok(Json(ApiResponse::success(
        sqlx::query_as("SELECT * FROM bridge_commands WHERE operation_id=? ORDER BY sequence")
            .bind(id)
            .fetch_all(&d.db().pool)
            .await?,
    )))
}

pub(super) async fn launch(
    pool: &sqlx::SqlitePool,
    op: &ResourceOperation,
    runtime: Uuid,
) -> anyhow::Result<()> {
    op.spec.runner.as_ref().context("Missing runner")?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    model::validate_target(&mut tx, &op.spec, op.workspace_id).await?;
    let changed = sqlx::query("UPDATE resource_operations SET status='running',updated_at=datetime('now','subsec') WHERE id=? AND runtime_id=? AND status='launching' AND cancel_requested=0")
        .bind(op.id).bind(runtime).execute(&mut *tx).await?.rows_affected();
    ensure!(changed == 1, "Remote dispatch cancelled or stale");
    sqlx::query("INSERT INTO bridge_commands(id,operation_id,kind,script) VALUES(?,?,'start',?)")
        .bind(op.id)
        .bind(op.id)
        .bind(&op.spec.script)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub(super) async fn reconcile(
    pool: &sqlx::SqlitePool,
    op: &ResourceOperation,
    runtime: Uuid,
) -> anyhow::Result<()> {
    let elapsed: i64 = sqlx::query_scalar("SELECT CAST((julianday('now')-julianday(updated_at))*86400 AS INTEGER) FROM resource_operations WHERE id=?").bind(op.id).fetch_one(pool).await?;
    let commands: Vec<Command> =
        sqlx::query_as("SELECT * FROM bridge_commands WHERE operation_id=? ORDER BY sequence")
            .bind(op.id)
            .fetch_all(pool)
            .await?;
    let failed = commands
        .iter()
        .any(|c| c.result.as_ref().is_some_and(|r| r["success"] != true));
    let completed = commands.iter().any(|c| {
        (c.kind == "finish" || !op.spec.runner.as_ref().is_some_and(|r| r.interactive))
            && c.result
                .as_ref()
                .is_some_and(|r| r["success"] == true && r["settled"] == true)
    });
    let stopped: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bridge_settlements WHERE operation_id=?)")
            .bind(op.id)
            .fetch_one(pool)
            .await?;
    if failed
        || op.cancel_requested
        || elapsed > i64::from(op.spec.timeout_seconds)
        || (stopped && !completed)
    {
        ResourceOperation::finish(pool,op.id,runtime,false,"Remote failure/cancellation/deadline/unexpected stop: ownership retained until runner settlement and operator recovery").await?;
    } else if completed {
        ResourceOperation::finish(
            pool,
            op.id,
            runtime,
            true,
            "Runner verified cleanup, process containment and declared resource state",
        )
        .await?;
    }
    Ok(())
}
pub(super) async fn require_settled(
    pool: &sqlx::SqlitePool,
    op: &ResourceOperation,
) -> anyhow::Result<()> {
    let settled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bridge_settlements WHERE operation_id=? AND runtime_id=?)",
    )
    .bind(op.id)
    .bind(op.runtime_id)
    .fetch_one(pool)
    .await?;
    ensure!(
        settled,
        "Runner has not confirmed containment is closed; reconnect the original runner and settle before operator recovery"
    );
    Ok(())
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new().nest(
        "/execution-bridge",
        Router::new()
            .route("/runners", get(runners).post(enroll))
            .route("/runners/{id}/disable", post(disable))
            .route("/runners/{id}/rotate-token", post(rotate_token))
            .route("/runners/{id}/poll", post(poll))
            .route(
                "/runners/{runner}/operations/{operation}/report",
                post(report),
            )
            .route(
                "/runners/{runner}/operations/{operation}/settled",
                post(settled),
            )
            .route("/sources", post(source::capture))
            .route("/sources/{id}", get(source::inspect))
            .route("/operations/{id}/commands", get(commands).post(enqueue))
            .layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
    )
}

#[cfg(test)]
mod tests {
    use db::models::{
        execution_bridge::RunnerTarget,
        resource_coordination::{
            RegisterResource, ResourceClaim, ResourceOperationSpec, SharedResource,
        },
    };

    use super::*;

    async fn fixture(interactive: bool) -> (sqlx::SqlitePool, ResourceOperation, Uuid) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
        let workspace = Uuid::new_v4();
        let session = Uuid::new_v4();
        sqlx::query("INSERT INTO workspaces(id,branch) VALUES(?,'bridge-test')")
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sessions(id,workspace_id) VALUES(?,?)")
            .bind(session)
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        let resource = SharedResource::register(
            &pool,
            RegisterResource {
                resource_key: "mock:slot".into(),
                name: "mock".into(),
                description: "Mock execution slot".into(),
                state: "idle".into(),
            },
        )
        .await
        .unwrap();
        let runner = Uuid::new_v4();
        let source = Uuid::new_v4();
        sqlx::query("INSERT INTO bridge_runners(id,name,token_hash,execution_resource_id,capabilities) VALUES(?,'mock','hash',?,'{\"protocol\":1}')").bind(runner).bind(resource.id).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO bridge_sources(id,workspace_id,digest,manifest) VALUES(?,?,'digest','{}')",
        )
        .bind(source)
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
        let spec = ResourceOperationSpec {
            request_id: Uuid::new_v4(),
            session_id: session,
            purpose: "mock bridge".into(),
            claims: vec![ResourceClaim {
                resource_id: resource.id,
                expected_revision: 1,
                resulting_state: None,
            }],
            script: "echo mock".into(),
            verification_script: "test ! -e busy".into(),
            working_dir: ".".into(),
            timeout_seconds: 60,
            runner: Some(RunnerTarget {
                runner_id: runner,
                source_id: source,
                interactive,
                desktop: false,
                cleanup_script: "rm -f busy".into(),
            }),
        };
        let op = ResourceOperation::submit(&pool, spec).await.unwrap();
        let runtime = Uuid::new_v4();
        ResourceOperation::allocate(&pool, runtime).await.unwrap();
        let op = ResourceOperation::find(&pool, op.id).await.unwrap();
        launch(&pool, &op, runtime).await.unwrap();
        // Simulate the durable sent transition, before delivery can leave LVK.
        sqlx::query("UPDATE bridge_commands SET status='sent' WHERE operation_id=?")
            .bind(op.id)
            .execute(&pool)
            .await
            .unwrap();
        let op = ResourceOperation::find(&pool, op.id).await.unwrap();
        (pool, op, runner)
    }
    fn receipt(op: &ResourceOperation, settled: bool) -> Report {
        Report {
            runtime_id: op.runtime_id.unwrap(),
            command_id: op.id,
            success: true,
            settled,
            output: "mock output".into(),
            artifacts: vec![],
        }
    }
    async fn holders(pool: &sqlx::SqlitePool, op: Uuid) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM resource_holders WHERE operation_id=?")
            .bind(op)
            .fetch_one(pool)
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn credentials_and_installation_are_scoped() {
        let (pool, _op, runner) = fixture(false).await;
        let installation = Uuid::new_v4();
        sqlx::query("UPDATE bridge_runners SET token_hash=?,installation_id=? WHERE id=?")
            .bind(hash("test-secret"))
            .bind(installation)
            .bind(runner)
            .execute(&pool)
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-lvk-runner-token", "test-secret".parse().unwrap());
        headers.insert(
            "x-lvk-runner-installation",
            installation.to_string().parse().unwrap(),
        );
        authenticate(&pool, runner, &headers).await.unwrap();
        assert!(authenticate(&pool, Uuid::new_v4(), &headers).await.is_err());
        headers.insert("x-lvk-runner-token", "wrong".parse().unwrap());
        assert!(authenticate(&pool, runner, &headers).await.is_err());
        headers.insert("x-lvk-runner-token", "test-secret".parse().unwrap());
        headers.insert(
            "x-lvk-runner-installation",
            Uuid::new_v4().to_string().parse().unwrap(),
        );
        assert!(authenticate(&pool, runner, &headers).await.is_err());
    }
    #[tokio::test]
    async fn one_shot_receipt_is_durable_idempotent_and_releases_only_verified() {
        let (pool, op, runner) = fixture(false).await;
        save_report(&pool, runner, op.id, receipt(&op, true))
            .await
            .unwrap();
        save_report(&pool, runner, op.id, receipt(&op, true))
            .await
            .unwrap();
        let mut changed = receipt(&op, true);
        changed.output = "different".into();
        assert!(save_report(&pool, runner, op.id, changed).await.is_err());
        assert!(
            save_report(&pool, Uuid::new_v4(), op.id, receipt(&op, true))
                .await
                .is_err()
        );
        let mut stale = receipt(&op, true);
        stale.runtime_id = Uuid::new_v4();
        assert!(save_report(&pool, runner, op.id, stale).await.is_err());
        reconcile(&pool, &op, op.runtime_id.unwrap()).await.unwrap();
        assert_eq!(
            ResourceOperation::find(&pool, op.id).await.unwrap().status,
            "succeeded"
        );
        assert_eq!(holders(&pool, op.id).await, 0);
    }
    #[tokio::test]
    async fn interactive_start_holds_and_unexpected_stop_requires_recovery() {
        let (pool, op, runner) = fixture(true).await;
        save_report(&pool, runner, op.id, receipt(&op, false))
            .await
            .unwrap();
        reconcile(&pool, &op, op.runtime_id.unwrap()).await.unwrap();
        assert_eq!(holders(&pool, op.id).await, 1);
        assert_eq!(
            ResourceOperation::find(&pool, op.id).await.unwrap().status,
            "running"
        );
        assert!(require_settled(&pool, &op).await.is_err());
        sqlx::query(
            "INSERT INTO bridge_settlements VALUES(?,?,'runner restarted and confirmed job empty')",
        )
        .bind(op.id)
        .bind(op.runtime_id)
        .execute(&pool)
        .await
        .unwrap();
        reconcile(&pool, &op, op.runtime_id.unwrap()).await.unwrap();
        assert_eq!(
            ResourceOperation::find(&pool, op.id).await.unwrap().status,
            "recovery_required"
        );
        assert_eq!(holders(&pool, op.id).await, 1);
        require_settled(&pool, &op).await.unwrap();
    }
    #[tokio::test]
    async fn cancellation_and_deadline_retain_even_with_late_success() {
        for cancelled in [true, false] {
            let (pool, op, runner) = fixture(false).await;
            save_report(&pool, runner, op.id, receipt(&op, true))
                .await
                .unwrap();
            if cancelled {
                ResourceOperation::cancel(&pool, op.id).await.unwrap();
            } else {
                sqlx::query("UPDATE resource_operations SET updated_at=datetime('now','-2 hours') WHERE id=?").bind(op.id).execute(&pool).await.unwrap();
            }
            let current = ResourceOperation::find(&pool, op.id).await.unwrap();
            reconcile(&pool, &current, op.runtime_id.unwrap())
                .await
                .unwrap();
            assert_eq!(
                ResourceOperation::find(&pool, op.id).await.unwrap().status,
                "recovery_required"
            );
            assert_eq!(holders(&pool, op.id).await, 1);
        }
    }
}
