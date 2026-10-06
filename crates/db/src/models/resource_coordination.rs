//! Durable exclusive resource bundles. Timeouts request cancellation; they
//! never revoke ownership. External adapters must honour the supplied fences.
use std::collections::HashSet;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection, SqlitePool, types::Json};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow, PartialEq, Eq)]
pub struct SharedResource {
    pub id: Uuid,
    pub resource_key: String,
    pub name: String,
    pub description: String,
    pub state: String,
    #[ts(type = "number")]
    pub revision: i64,
    #[ts(type = "number")]
    pub fence: i64,
    pub health: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RegisterResource {
    pub resource_key: String,
    pub name: String,
    pub description: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceClaim {
    pub resource_id: Uuid,
    /// The contract/state observed before authoring this operation.
    #[ts(type = "number")]
    pub expected_revision: i64,
    /// Persistent state after success, e.g. schema v3; None means unchanged.
    pub resulting_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceOperationSpec {
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub purpose: String,
    pub claims: Vec<ResourceClaim>,
    /// Entire critical section, including cleanup. Do not detach work.
    pub script: String,
    /// Independently checks that all resources are idle in the declared state.
    pub verification_script: String,
    /// Relative to the multi-repository workspace root.
    pub working_dir: String,
    pub timeout_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow)]
pub struct ResourceOperation {
    #[ts(type = "number")]
    pub sequence: i64,
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub session_id: Uuid,
    #[ts(type = "ResourceOperationSpec")]
    pub spec: Json<ResourceOperationSpec>,
    pub status: String,
    #[ts(type = "number")]
    pub priority: i64,
    pub process_id: Option<Uuid>,
    pub runtime_id: Option<Uuid>,
    pub cancel_requested: bool,
    pub message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow, PartialEq, Eq)]
pub struct ResourceHolder {
    pub resource_id: Uuid,
    pub operation_id: Uuid,
    #[ts(type = "number")]
    pub fence: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow)]
pub struct ResourceEvent {
    #[ts(type = "number")]
    pub sequence: i64,
    pub resource_id: Option<Uuid>,
    pub operation_id: Option<Uuid>,
    pub kind: String,
    pub message: String,
    pub created_at: String,
}

/// Deliberately excludes scripts, credentials and private agent transcripts.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct ResourceQueueEntry {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub purpose: String,
    pub claims: Vec<ResourceClaim>,
    pub status: String,
    #[ts(type = "number")]
    pub priority: i64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub resources: Vec<SharedResource>,
    pub holders: Vec<ResourceHolder>,
    pub queue: Vec<ResourceQueueEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow)]
pub struct ResourceMediation {
    pub id: Uuid,
    pub trigger_operation_id: Uuid,
    #[ts(type = "ResourceSnapshot")]
    pub snapshot: Json<ResourceSnapshot>,
    pub status: String,
    pub workspace_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub agent_run_id: Option<Uuid>,
    pub result: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ResourceDecision {
    pub explanation: String,
    /// A complete ordering of the snapshot's queued requests. Never preempts.
    pub order: Vec<Uuid>,
    /// Queued requests whose authors must reconsider their assumptions.
    pub needs_replan: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ResourceRecovery {
    pub evidence: String,
    /// All held resources, with current revisions and verified resulting states.
    pub claims: Vec<ResourceClaim>,
}

fn bounded(value: &str, max: usize, label: &str) -> anyhow::Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= max && !value.contains('\0'),
        "Invalid {label}"
    );
    Ok(())
}

pub fn validate_spec(spec: &ResourceOperationSpec) -> anyhow::Result<()> {
    bounded(&spec.purpose, 4000, "purpose")?;
    bounded(&spec.script, 65536, "script")?;
    bounded(&spec.verification_script, 16384, "verification script")?;
    ensure!(
        !matches!(spec.verification_script.trim(), "true" | ":" | "exit 0"),
        "Provide an actual resource idle/state check"
    );
    ensure!(
        (1..=3600).contains(&spec.timeout_seconds),
        "Timeout must be 1–3600 seconds"
    );
    ensure!(
        !spec.claims.is_empty() && spec.claims.len() <= 16,
        "Claim 1–16 resources atomically"
    );
    let ids: HashSet<_> = spec.claims.iter().map(|c| c.resource_id).collect();
    ensure!(ids.len() == spec.claims.len(), "Duplicate resource claim");
    for c in &spec.claims {
        ensure!(
            c.expected_revision > 0,
            "Observe each resource revision before requesting it"
        );
        if let Some(state) = &c.resulting_state {
            bounded(state, 4000, "resulting state")?;
        }
    }
    let path = &spec.working_dir;
    ensure!(
        !std::path::Path::new(path).is_absolute()
            && !path.contains(['\\', ':', '\0'])
            && !path.split('/').any(|p| p == ".."),
        "Working directory must stay inside the workspace"
    );
    Ok(())
}

async fn event(
    conn: &mut SqliteConnection,
    resource: Option<Uuid>,
    operation: Option<Uuid>,
    kind: &str,
    message: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO resource_events(resource_id,operation_id,kind,message) VALUES(?,?,?,?)",
    )
    .bind(resource)
    .bind(operation)
    .bind(kind)
    .bind(message)
    .execute(conn)
    .await?;
    Ok(())
}

impl SharedResource {
    pub async fn register(pool: &SqlitePool, request: RegisterResource) -> anyhow::Result<Self> {
        bounded(&request.resource_key, 256, "resource key")?;
        ensure!(
            request.resource_key.trim() == request.resource_key,
            "Resource key cannot contain surrounding whitespace"
        );
        bounded(&request.name, 200, "name")?;
        bounded(&request.description, 4000, "resource usage contract")?;
        bounded(&request.state, 4000, "state")?;
        let id = Uuid::new_v4();
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("INSERT INTO shared_resources(id,resource_key,name,description,state) VALUES(?,?,?,?,?)")
            .bind(id).bind(request.resource_key).bind(request.name).bind(request.description).bind(request.state).execute(&mut *tx).await?;
        event(
            &mut tx,
            Some(id),
            None,
            "registered",
            "Exclusive resource registered; use one canonical key for the physical resource",
        )
        .await?;
        let resource = sqlx::query_as("SELECT * FROM shared_resources WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(resource)
    }
}

async fn snapshot_conn(conn: &mut SqliteConnection) -> anyhow::Result<ResourceSnapshot> {
    let resources = sqlx::query_as("SELECT * FROM shared_resources ORDER BY resource_key")
        .fetch_all(&mut *conn)
        .await?;
    let holders = sqlx::query_as("SELECT * FROM resource_holders ORDER BY resource_id")
        .fetch_all(&mut *conn)
        .await?;
    let operations: Vec<ResourceOperation> = sqlx::query_as("SELECT * FROM resource_operations WHERE status IN ('queued','blocked','launching','running','recovery_required') ORDER BY priority DESC,sequence").fetch_all(conn).await?;
    let queue = operations
        .into_iter()
        .map(|o| ResourceQueueEntry {
            id: o.id,
            workspace_id: o.workspace_id,
            purpose: o.spec.purpose.clone(),
            claims: o.spec.claims.clone(),
            status: o.status,
            priority: o.priority,
            message: o.message,
        })
        .collect();
    Ok(ResourceSnapshot {
        resources,
        holders,
        queue,
    })
}

pub async fn snapshot(pool: &SqlitePool) -> anyhow::Result<ResourceSnapshot> {
    let mut tx = pool.begin().await?;
    let snapshot = snapshot_conn(&mut tx).await?;
    tx.commit().await?;
    Ok(snapshot)
}

impl ResourceOperation {
    pub async fn find(pool: &SqlitePool, id: Uuid) -> anyhow::Result<Self> {
        Ok(
            sqlx::query_as("SELECT * FROM resource_operations WHERE id=?")
                .bind(id)
                .fetch_one(pool)
                .await?,
        )
    }

    pub async fn submit(
        pool: &SqlitePool,
        mut spec: ResourceOperationSpec,
    ) -> anyhow::Result<Self> {
        validate_spec(&spec)?;
        spec.claims.sort_by_key(|c| c.resource_id);
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) =
            sqlx::query_as::<_, Self>("SELECT * FROM resource_operations WHERE id=?")
                .bind(spec.request_id)
                .fetch_optional(&mut *tx)
                .await?
        {
            ensure!(
                existing.spec.0 == spec,
                "Request ID already used with different content"
            );
            return Ok(existing);
        }
        let (workspace_id, archived, deleted): (Uuid,bool,bool) = sqlx::query_as("SELECT w.id,w.archived,w.worktree_deleted FROM sessions s JOIN workspaces w ON w.id=s.workspace_id WHERE s.id=? AND w.usage='interactive'")
            .bind(spec.session_id).fetch_optional(&mut *tx).await?.context("Choose an interactive workspace session")?;
        ensure!(!archived && !deleted, "Workspace is archived or deleted");
        let reserved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integration_reservations WHERE resource_kind='workspace' AND resource_key=lower(hex(?)))")
            .bind(workspace_id).fetch_one(&mut *tx).await?;
        ensure!(!reserved, "Workspace is reserved by Integration");
        for claim in &spec.claims {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM shared_resources WHERE id=?)")
                    .bind(claim.resource_id)
                    .fetch_one(&mut *tx)
                    .await?;
            ensure!(exists, "Unknown resource {}", claim.resource_id);
        }
        let id = spec.request_id;
        sqlx::query(
            "INSERT INTO resource_operations(id,workspace_id,session_id,spec) VALUES(?,?,?,?)",
        )
        .bind(id)
        .bind(workspace_id)
        .bind(spec.session_id)
        .bind(Json(spec))
        .execute(&mut *tx)
        .await?;
        event(
            &mut tx,
            None,
            Some(id),
            "queued",
            "Operation queued; no resource is held while waiting",
        )
        .await?;
        tx.commit().await?;
        Self::find(pool, id).await
    }

    /// All grants and their monotonically increasing fences commit together.
    /// An older conflicting request prevents overtaking; disjoint work proceeds.
    pub async fn allocate(pool: &SqlitePool, runtime_id: Uuid) -> anyhow::Result<Vec<Uuid>> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut view = snapshot_conn(&mut tx).await?;
        let mut unavailable: HashSet<_> = view.holders.iter().map(|h| h.resource_id).collect();
        let mut granted = Vec::new();
        for op in view.queue.iter().filter(|o| o.status == "queued") {
            let mismatch = op.claims.iter().find(|c| {
                view.resources
                    .iter()
                    .all(|r| r.id != c.resource_id || r.revision != c.expected_revision)
            });
            if let Some(claim) = mismatch {
                let message = format!(
                    "Resource {} changed state/contract; inspect the new revision and submit a new request",
                    claim.resource_id
                );
                sqlx::query("UPDATE resource_operations SET status='blocked',message=?,updated_at=datetime('now','subsec') WHERE id=?")
                    .bind(&message).bind(op.id).execute(&mut *tx).await?;
                event(
                    &mut tx,
                    Some(claim.resource_id),
                    Some(op.id),
                    "needs_replan",
                    &message,
                )
                .await?;
                continue;
            }
            if op.claims.iter().any(|c| {
                unavailable.contains(&c.resource_id)
                    || view
                        .resources
                        .iter()
                        .any(|r| r.id == c.resource_id && r.health != "ready")
            }) {
                unavailable.extend(op.claims.iter().map(|c| c.resource_id));
                continue;
            }
            for claim in &op.claims {
                let resource = view
                    .resources
                    .iter_mut()
                    .find(|r| r.id == claim.resource_id)
                    .context("Resource disappeared")?;
                resource.fence += 1;
                sqlx::query("UPDATE shared_resources SET fence=? WHERE id=?")
                    .bind(resource.fence)
                    .bind(resource.id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("INSERT INTO resource_holders VALUES(?,?,?)")
                    .bind(resource.id)
                    .bind(op.id)
                    .bind(resource.fence)
                    .execute(&mut *tx)
                    .await?;
                unavailable.insert(resource.id);
            }
            sqlx::query("UPDATE resource_operations SET status='launching',runtime_id=?,updated_at=datetime('now','subsec') WHERE id=?")
                .bind(runtime_id).bind(op.id).execute(&mut *tx).await?;
            event(
                &mut tx,
                None,
                Some(op.id),
                "acquired",
                "Entire resource bundle acquired",
            )
            .await?;
            granted.push(op.id);
        }
        tx.commit().await?;
        Ok(granted)
    }

    pub async fn mark_running(
        pool: &SqlitePool,
        id: Uuid,
        runtime: Uuid,
        process: Uuid,
    ) -> anyhow::Result<()> {
        let changed = sqlx::query("UPDATE resource_operations SET status='running',process_id=?,updated_at=datetime('now','subsec') WHERE id=? AND runtime_id=? AND status='launching'")
            .bind(process).bind(id).bind(runtime).execute(pool).await?.rows_affected();
        ensure!(changed == 1, "Operation no longer owns this dispatch");
        Ok(())
    }

    pub async fn cancel(pool: &SqlitePool, id: Uuid) -> anyhow::Result<()> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed=sqlx::query("UPDATE resource_operations SET cancel_requested=1,status=CASE WHEN status IN ('queued','blocked') THEN 'cancelled' ELSE status END,updated_at=datetime('now','subsec') WHERE id=? AND cancel_requested=0 AND status IN ('queued','blocked','launching','running','recovery_required')")
            .bind(id).execute(&mut *tx).await?.rows_affected();
        if changed == 1 {
            event(
                &mut tx,
                None,
                Some(id),
                "cancel_requested",
                "Cancellation does not release an active resource bundle",
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Called only by the host after command + verification + process cleanup.
    pub async fn finish(
        pool: &SqlitePool,
        id: Uuid,
        runtime: Uuid,
        verified: bool,
        message: &str,
    ) -> anyhow::Result<()> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let op: Self = sqlx::query_as("SELECT * FROM resource_operations WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        ensure!(
            op.runtime_id == Some(runtime) && matches!(op.status.as_str(), "launching" | "running"),
            "Stale completion/dispatch token"
        );
        let holders: Vec<ResourceHolder> =
            sqlx::query_as("SELECT * FROM resource_holders WHERE operation_id=?")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        ensure!(
            holders.len() == op.spec.claims.len(),
            "Resource ownership is incomplete"
        );
        let success = verified && !op.cancel_requested;
        for claim in &op.spec.claims {
            let holder = holders
                .iter()
                .find(|h| h.resource_id == claim.resource_id)
                .context("Resource owner mismatch")?;
            let fence: i64 = sqlx::query_scalar("SELECT fence FROM shared_resources WHERE id=?")
                .bind(claim.resource_id)
                .fetch_one(&mut *tx)
                .await?;
            ensure!(holder.fence == fence, "Stale resource fence");
            if success {
                if let Some(state) = &claim.resulting_state {
                    sqlx::query(
                        "UPDATE shared_resources SET state=?,revision=revision+1 WHERE id=?",
                    )
                    .bind(state)
                    .bind(claim.resource_id)
                    .execute(&mut *tx)
                    .await?;
                    event(
                        &mut tx,
                        Some(claim.resource_id),
                        Some(id),
                        "state_changed",
                        state,
                    )
                    .await?;
                }
            } else {
                sqlx::query("UPDATE shared_resources SET health='recovery_required' WHERE id=?")
                    .bind(claim.resource_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        let status = if success {
            "succeeded"
        } else {
            "recovery_required"
        };
        if success {
            sqlx::query("DELETE FROM resource_holders WHERE operation_id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE resource_operations SET status=?,message=?,updated_at=datetime('now','subsec') WHERE id=?")
            .bind(status).bind(message).bind(id).execute(&mut *tx).await?;
        event(&mut tx, None, Some(id), status, message).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deliberate operator confirmation, never an AI decision or lease timeout.
    pub async fn recover(
        pool: &SqlitePool,
        id: Uuid,
        recovery: ResourceRecovery,
    ) -> anyhow::Result<()> {
        bounded(&recovery.evidence, 4000, "recovery evidence")?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let op: Self = sqlx::query_as("SELECT * FROM resource_operations WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        ensure!(
            op.status == "recovery_required",
            "Operation does not require recovery"
        );
        let holders: Vec<ResourceHolder> =
            sqlx::query_as("SELECT * FROM resource_holders WHERE operation_id=?")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        let unique: HashSet<_> = recovery.claims.iter().map(|c| c.resource_id).collect();
        ensure!(
            holders.len() == recovery.claims.len() && unique.len() == holders.len(),
            "Confirm every held resource exactly once"
        );
        for claim in &recovery.claims {
            let holder = holders
                .iter()
                .find(|h| h.resource_id == claim.resource_id)
                .context("Resource not held by this operation")?;
            let state = claim
                .resulting_state
                .as_deref()
                .context("Record the verified current state")?;
            bounded(state, 4000, "verified state")?;
            let changed = sqlx::query("UPDATE shared_resources SET health='ready',state=?,revision=revision+1 WHERE id=? AND revision=? AND fence=?")
                .bind(state).bind(claim.resource_id).bind(claim.expected_revision).bind(holder.fence).execute(&mut *tx).await?.rows_affected();
            ensure!(changed == 1, "Recovery observation is stale");
            event(
                &mut tx,
                Some(claim.resource_id),
                Some(id),
                "recovered",
                &recovery.evidence,
            )
            .await?;
        }
        sqlx::query("DELETE FROM resource_holders WHERE operation_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE resource_operations SET status='recovered',message=?,updated_at=datetime('now','subsec') WHERE id=?")
            .bind(recovery.evidence).bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

impl ResourceMediation {
    pub async fn fence_interrupted_preparations(pool: &SqlitePool) -> anyhow::Result<()> {
        sqlx::query("UPDATE resource_mediations SET status='failed',result='Preparation interrupted; inspect retained execution. No automatic redispatch.' WHERE status='preparing'").execute(pool).await?;
        Ok(())
    }
    pub async fn request(
        pool: &SqlitePool,
        operation: Uuid,
        explicit: bool,
    ) -> anyhow::Result<Option<Self>> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let seen: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM resource_mediations WHERE trigger_operation_id=?)",
        )
        .bind(operation)
        .fetch_one(&mut *tx)
        .await?;
        if seen && !explicit {
            return Ok(None);
        }
        let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resource_mediations WHERE status IN ('pending','preparing','running')) OR EXISTS(SELECT 1 FROM agent_runs a JOIN resource_mediations m ON m.id=a.correlation_id WHERE a.status NOT IN ('succeeded','failed','cancelled','crashed','audit_failed'))").fetch_one(&mut *tx).await?;
        if active {
            return Ok(None);
        }
        let snapshot = snapshot_conn(&mut tx).await?;
        ensure!(
            snapshot.queue.iter().any(|o| o.id == operation
                && matches!(
                    o.status.as_str(),
                    "queued" | "blocked" | "recovery_required"
                )),
            "Operation does not need mediation"
        );
        if !explicit {
            let target = snapshot
                .queue
                .iter()
                .find(|o| o.id == operation)
                .context("Operation disappeared")?;
            let needs_judgement = target.status != "queued"
                || target.claims.iter().any(|claim| {
                    snapshot
                        .holders
                        .iter()
                        .any(|h| h.resource_id == claim.resource_id)
                        || snapshot.resources.iter().any(|r| {
                            r.id == claim.resource_id
                                && (r.health != "ready" || r.revision != claim.expected_revision)
                        })
                        || snapshot.queue.iter().any(|o| {
                            o.id != operation
                                && o.status == "queued"
                                && o.claims.iter().any(|c| c.resource_id == claim.resource_id)
                        })
                });
            if !needs_judgement {
                return Ok(None);
            }
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO resource_mediations(id,trigger_operation_id,snapshot,status) VALUES(?,?,?,'pending')")
            .bind(id).bind(operation).bind(Json(snapshot)).execute(&mut *tx).await?;
        let result = sqlx::query_as("SELECT * FROM resource_mediations WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(result))
    }

    pub async fn apply(
        pool: &SqlitePool,
        id: Uuid,
        decision: ResourceDecision,
    ) -> anyhow::Result<bool> {
        bounded(&decision.explanation, 8000, "decision explanation")?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let mediation: Self = sqlx::query_as("SELECT * FROM resource_mediations WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        ensure!(mediation.status == "running", "Mediation is not running");
        let current = snapshot_conn(&mut tx).await?;
        let fresh = current == mediation.snapshot.0;
        if fresh {
            let pending: HashSet<_> = current
                .queue
                .iter()
                .filter(|o| o.status == "queued")
                .map(|o| o.id)
                .collect();
            let ordered: HashSet<_> = decision.order.iter().copied().collect();
            let blocked: HashSet<_> = decision.needs_replan.iter().copied().collect();
            ensure!(
                pending == ordered
                    && ordered.len() == decision.order.len()
                    && blocked.len() == decision.needs_replan.len()
                    && blocked.is_subset(&pending),
                "Decision must order the entire pending queue and may only hold queued requests"
            );
            for (index, op) in decision.order.iter().enumerate() {
                sqlx::query("UPDATE resource_operations SET priority=?,status=CASE WHEN ? THEN 'blocked' ELSE status END,message=?,updated_at=datetime('now','subsec') WHERE id=? AND status='queued'")
                    .bind((decision.order.len()-index) as i64).bind(blocked.contains(op)).bind(&decision.explanation).bind(op).execute(&mut *tx).await?;
            }
        }
        let result = serde_json::to_string(&decision)?;
        sqlx::query("UPDATE resource_mediations SET status=?,result=? WHERE id=?")
            .bind(if fresh { "applied" } else { "stale" })
            .bind(&result)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        event(
            &mut tx,
            None,
            Some(mediation.trigger_operation_id),
            if fresh { "mediated" } else { "stale_decision" },
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(fresh)
    }
}

#[cfg(test)]
mod tests;
