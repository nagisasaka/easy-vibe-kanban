//! Formal Integration product state. Durable reservations are independent of
//! expiring Orchestration dispatcher leases. No conversation/semantic database.
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntegrationSelection {
    pub card_id: Uuid,
    pub workspace_id: Uuid,
    // HEAD observed when the user adopted this Workspace, not a moving ref.
    pub expected_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntegrationSource {
    pub selection: IntegrationSelection,
    pub branch: String,
    pub commit: String,
    pub title: String,
    pub description: Option<String>,
    pub status_id: Uuid,
    pub requirements_revision: i64,
    pub event_ids: Vec<Uuid>,
    // All current Card workspaces are reserved, not only the adopted one.
    pub related_workspaces: Vec<Uuid>,
    pub done_result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntegrationValidation {
    pub command: String,
    // Relative to the repository root, not the multi-repository container.
    pub cwd: String,
    pub required: bool,
    pub evidence: String,
    pub environment_requirements: String,
    pub execution_process_id: Option<Uuid>,
    pub exit_code: Option<i64>,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct IntegrationPayload {
    pub sources: Vec<IntegrationSource>,
    pub observed_target: String,
    pub base_commit: Option<String>,
    pub result_commit: Option<String>,
    pub validation: Vec<IntegrationValidation>,
    pub exclusions: Vec<String>,
    pub semantic_summary: Option<String>,
    pub semantics: Option<utils::repository_memory::SemanticChanges>,
    pub integration_manifest: Option<utils::repository_memory::ChangeManifest>,
    pub publication_intent: bool,
    pub published: bool,
    pub wiki_result: Option<String>,
    pub executor_config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, FromRow)]
pub struct IntegrationRun {
    pub id: Uuid,
    pub request_key: String,
    pub project_id: Uuid,
    pub repository_id: Uuid,
    pub storage_identity: String,
    pub target_ref: String,
    pub status: String,
    pub workspace_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub agent_run_id: Option<Uuid>,
    #[ts(type = "IntegrationPayload")]
    pub payload: sqlx::types::Json<IntegrationPayload>,
    pub cancel_requested: bool,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn resource_key(id: Uuid) -> String {
    id.simple().to_string()
}

pub async fn workspace_owner(pool: &SqlitePool, id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT run_id FROM integration_reservations WHERE resource_kind='workspace' AND resource_key=?")
        .bind(resource_key(id)).fetch_optional(pool).await
}

pub async fn guard_workspace(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    if let Some(run) = workspace_owner(pool, id).await? {
        return Err(sqlx::Error::Protocol(format!(
            "Workspace reserved by Integration {run}; inspect /api/integrations/{run}"
        )));
    }
    Ok(())
}

pub async fn is_integration_workspace(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integration_runs WHERE workspace_id=?)")
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn guard_agent_dispatch(
    pool: &SqlitePool,
    workspace: Uuid,
    session: Uuid,
    correlation: Uuid,
) -> Result<(), sqlx::Error> {
    let product: Option<(Uuid, Option<Uuid>, String, bool)> = sqlx::query_as(
        "SELECT id,session_id,status,cancel_requested FROM integration_runs WHERE workspace_id=?",
    )
    .bind(workspace)
    .fetch_optional(pool)
    .await?;
    let owner = workspace_owner(pool, workspace).await?;
    if let Some((id, expected_session, status, cancelled)) = product {
        if owner != Some(id)
            || correlation != id
            || expected_session != Some(session)
            || cancelled
            || !matches!(status.as_str(), "preparing" | "integrating")
        {
            return Err(sqlx::Error::Protocol(format!(
                "Integration {id} does not permit new Agent dispatch in phase {status}; use Board progress/Cancel or a new explicit Integration"
            )));
        }
    } else if let Some(owner) = owner {
        return Err(sqlx::Error::Protocol(format!(
            "Source Workspace reserved by Integration {owner}"
        )));
    }
    Ok(())
}

impl IntegrationRun {
    /// Execute before the generic Orchestration outbox is redelivered. A
    /// half-prepared product must never start a new child just because its
    /// dispatcher lease disappeared. Keep reservations until stop confirmation.
    pub async fn fence_interrupted_preparations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE integration_runs SET status='cancelling',error='Preparation interrupted by service restart; stop any accepted child before releasing reservations and start a new Integration',updated_at=datetime('now','subsec') WHERE status='preparing'")
            .execute(pool).await?;
        Ok(())
    }
    pub async fn request_cancel(pool: &SqlitePool, id: Uuid) -> Result<Self, sqlx::Error> {
        sqlx::query("UPDATE integration_runs SET cancel_requested=1,updated_at=datetime('now','subsec') WHERE id=? AND status IN ('queued','preparing','integrating','validating','cancelling')")
            .bind(id).execute(pool).await?;
        Self::find(pool, id).await
    }

    /// Used inside the same writer transaction as target reservations. Exactly
    /// one of accepted cancellation or publication intent can win.
    pub async fn record_publication_intent(
        connection: &mut sqlx::SqliteConnection,
        run: &Self,
    ) -> Result<(), sqlx::Error> {
        if !run.payload.publication_intent || run.payload.published {
            return Err(sqlx::Error::Protocol("Invalid publication intent".into()));
        }
        let changed=sqlx::query("UPDATE integration_runs SET status='publishing',payload=?,updated_at=datetime('now','subsec') WHERE id=? AND cancel_requested=0 AND status='validating'")
            .bind(&run.payload).bind(run.id).execute(connection).await?.rows_affected();
        if changed != 1 {
            return Err(sqlx::Error::Protocol(
                "Cancelled or ownership changed before publication".into(),
            ));
        }
        Ok(())
    }
    pub async fn find(pool: &SqlitePool, id: Uuid) -> Result<Self, sqlx::Error> {
        sqlx::query_as("SELECT * FROM integration_runs WHERE id=?")
            .bind(id)
            .fetch_one(pool)
            .await
    }
    pub async fn save(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        // Cancellation is never overwritten by a worker's older observation.
        sqlx::query("UPDATE integration_runs SET status=?,workspace_id=?,session_id=?,agent_run_id=?,payload=?,error=?,updated_at=datetime('now','subsec') WHERE id=?")
            .bind(&self.status).bind(self.workspace_id).bind(self.session_id).bind(self.agent_run_id)
            .bind(&self.payload).bind(&self.error).bind(self.id).execute(pool).await?;
        Ok(())
    }
    /// Card effect and its durable receipt commit together. Reload under the
    /// SQLite writer lock, so concurrent recovery cannot re-Done a reopened Card.
    pub async fn complete_cards(pool: &SqlitePool, id: Uuid) -> Result<Self, sqlx::Error> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut run: Self = sqlx::query_as("SELECT * FROM integration_runs WHERE id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if !run.payload.published {
            return Err(sqlx::Error::Protocol(
                "Cannot update Cards before confirmed Git publication".into(),
            ));
        }
        let done:Option<Uuid>=sqlx::query_scalar("SELECT id FROM local_project_statuses WHERE project_id=? AND lower(name)='done' AND hidden=0 ORDER BY sort_order,id LIMIT 1")
            .bind(run.project_id).fetch_optional(&mut *tx).await?;
        for source in &mut run.payload.sources {
            if source.done_result.is_some() {
                continue;
            }
            let owner:Option<Uuid>=sqlx::query_scalar("SELECT run_id FROM integration_reservations WHERE resource_kind='card' AND resource_key=?")
                .bind(resource_key(source.selection.card_id)).fetch_optional(&mut *tx).await?;
            if owner.is_some_and(|owner| owner != id) {
                source.done_result =
                    Some("not changed: Card reserved by another Integration".into());
                continue;
            }
            sqlx::query("DELETE FROM integration_reservations WHERE run_id=? AND resource_kind='card' AND resource_key=?")
                .bind(id).bind(resource_key(source.selection.card_id)).execute(&mut *tx).await?;
            let count = if let Some(done) = done {
                sqlx::query("UPDATE local_issues SET status_id=?,completed_at=datetime('now','subsec'),updated_at=datetime('now','subsec') WHERE id=? AND project_id=? AND title=? AND description IS ? AND status_id=? AND integration_revision=? AND EXISTS(SELECT 1 FROM local_workspace_links WHERE issue_id=? AND workspace_id=?)")
                    .bind(done).bind(source.selection.card_id).bind(run.project_id).bind(&source.title).bind(&source.description).bind(source.status_id).bind(source.requirements_revision).bind(source.selection.card_id).bind(source.selection.workspace_id).execute(&mut *tx).await?.rows_affected()
            } else {
                0
            };
            source.done_result = Some(
                if count == 1 {
                    "done"
                } else {
                    "not changed: Card requirements/link/status or Done configuration changed"
                }
                .into(),
            );
        }
        sqlx::query(
            "UPDATE integration_runs SET payload=?,updated_at=datetime('now','subsec') WHERE id=?",
        )
        .bind(&run.payload)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        repo::Repo,
        session::{CreateSession, Session},
        workspace::{CreateWorkspace, Workspace},
    };

    async fn fixture() -> (SqlitePool, IntegrationRun, Uuid) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let project = Uuid::new_v4();
        let card = Uuid::new_v4();
        let todo = Uuid::new_v4();
        let done = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name) VALUES(?,'test')")
            .bind(project)
            .execute(&pool)
            .await
            .unwrap();
        for (id, name) in [(todo, "In Progress"), (done, "Done")] {
            sqlx::query("INSERT INTO local_project_statuses(id,project_id,name,color,sort_order) VALUES(?,?,?,'blue',0)").bind(id).bind(project).bind(name).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO local_issues(id,project_id,issue_number,simple_id,status_id,title,sort_order) VALUES(?,?,1,'TEST-1',?,'request',0)").bind(card).bind(project).bind(todo).execute(&pool).await.unwrap();
        let ws = Workspace::create(
            &pool,
            &CreateWorkspace {
                branch: "source".into(),
                name: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap();
        Session::create(
            &pool,
            &CreateSession {
                executor: None,
                name: None,
            },
            Uuid::new_v4(),
            ws.id,
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO local_workspace_links(workspace_id,issue_id,project_id) VALUES(?,?,?)",
        )
        .bind(ws.id)
        .bind(card)
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
        let repo = Repo::find_or_create(
            &pool,
            std::path::Path::new("/test-integration-fixture"),
            "test",
        )
        .await
        .unwrap();
        let id = Uuid::new_v4();
        let payload = IntegrationPayload {
            sources: vec![IntegrationSource {
                selection: IntegrationSelection {
                    card_id: card,
                    workspace_id: ws.id,
                    expected_commit: "a".repeat(40),
                },
                branch: "source".into(),
                commit: "a".repeat(40),
                title: "request".into(),
                description: None,
                status_id: todo,
                requirements_revision: 1,
                event_ids: vec![],
                related_workspaces: vec![ws.id],
                done_result: None,
            }],
            ..Default::default()
        };
        sqlx::query("INSERT INTO integration_runs(id,request_key,project_id,repository_id,storage_identity,target_ref,status,payload) VALUES(?,?,?,?, '/git/common','refs/heads/test','queued',?)")
            .bind(id).bind(id.to_string()).bind(project).bind(repo.id).bind(sqlx::types::Json(payload)).execute(&pool).await.unwrap();
        for (kind, id) in [("card", card), ("workspace", ws.id)] {
            sqlx::query("INSERT INTO integration_reservations VALUES(?,?,?)")
                .bind(kind)
                .bind(resource_key(id))
                .bind(
                    sqlx::query_scalar::<_, Uuid>("SELECT id FROM integration_runs")
                        .fetch_one(&pool)
                        .await
                        .unwrap(),
                )
                .execute(&pool)
                .await
                .unwrap();
        }
        (
            pool.clone(),
            IntegrationRun::find(&pool, id).await.unwrap(),
            done,
        )
    }

    #[tokio::test]
    async fn reservations_survive_dispatcher_restart_and_fence_real_mutations() {
        let (pool, run, _) = fixture().await;
        let s = &run.payload.sources[0];
        crate::models::orchestration::OrchestrationLeaseRecord::reconcile_startup(&pool)
            .await
            .unwrap();
        assert_eq!(
            workspace_owner(&pool, s.selection.workspace_id)
                .await
                .unwrap(),
            Some(run.id)
        );
        assert!(
            sqlx::query("UPDATE local_issues SET title='changed' WHERE id=?")
                .bind(s.selection.card_id)
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("DELETE FROM local_workspace_links WHERE workspace_id=?")
                .bind(s.selection.workspace_id)
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE workspaces SET worktree_deleted=1 WHERE id=?")
                .bind(s.selection.workspace_id)
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            Session::create(
                &pool,
                &CreateSession {
                    executor: None,
                    name: None
                },
                Uuid::new_v4(),
                s.selection.workspace_id
            )
            .await
            .is_err()
        );
        assert!(IntegrationRun::complete_cards(&pool, run.id).await.is_err());
        assert!(
            sqlx::query("INSERT INTO integration_reservations VALUES('workspace',?,?)")
                .bind(resource_key(s.selection.workspace_id))
                .bind(run.id)
                .execute(&pool)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cancellation_fences_a_late_agent_insert_at_the_database_boundary() {
        let (pool, mut run, _) = fixture().await;
        run.workspace_id = Some(run.payload.sources[0].selection.workspace_id);
        run.session_id = sqlx::query_scalar("SELECT id FROM sessions WHERE workspace_id=?")
            .bind(run.workspace_id)
            .fetch_optional(&pool)
            .await
            .unwrap();
        run.status = "preparing".into();
        run.save(&pool).await.unwrap();
        guard_agent_dispatch(
            &pool,
            run.workspace_id.unwrap(),
            run.session_id.unwrap(),
            run.id,
        )
        .await
        .unwrap();
        // Cancellation wins after the optimistic guard, before the actual INSERT.
        IntegrationRun::request_cancel(&pool, run.id).await.unwrap();
        let result=sqlx::query("INSERT INTO agent_runs(id,session_id,workspace_id,request_id,idempotency_key,correlation_id,schema_version,payload_version,runtime_profile_id,provider_id,workspace_mode,workspace_path,request_envelope) VALUES(?,?,?,?,?,?,1,1,'CODEX','codex','isolated_worktree','/fixture','{}')")
            .bind(Uuid::new_v4()).bind(run.session_id).bind(run.workspace_id).bind(Uuid::new_v4()).bind("late-dispatch").bind(run.id).execute(&pool).await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("reserved by formal Integration")
        );
        assert_eq!(
            workspace_owner(&pool, run.workspace_id.unwrap())
                .await
                .unwrap(),
            Some(run.id)
        );
    }

    #[tokio::test]
    async fn reserved_sources_reject_goal_reactivation_and_script_admission_until_release() {
        let (pool, run, _) = fixture().await;
        let workspace = run.payload.sources[0].selection.workspace_id;
        let session: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE workspace_id=?")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM integration_reservations WHERE run_id=?")
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();
        let agent = Uuid::new_v4();
        sqlx::query("INSERT INTO agent_runs(id,session_id,workspace_id,request_id,idempotency_key,correlation_id,schema_version,payload_version,runtime_profile_id,provider_id,workspace_mode,workspace_path,status,request_envelope) VALUES(?,?,?,?,?,?,1,1,'CODEX','codex','isolated_worktree','/fixture','succeeded','{}')")
            .bind(agent).bind(session).bind(workspace).bind(Uuid::new_v4())
            .bind(agent.to_string()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO integration_reservations VALUES('workspace',?,?)")
            .bind(resource_key(workspace))
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();
        for status in [
            "pending",
            "starting",
            "running",
            "awaiting_input",
            "awaiting_approval",
        ] {
            let result = sqlx::query("UPDATE agent_runs SET status=? WHERE id=?")
                .bind(status)
                .bind(agent)
                .execute(&pool)
                .await;
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("reserved by formal Integration")
            );
        }
        use executors::actions::{
            ExecutorAction, ExecutorActionType,
            script::{ScriptContext, ScriptRequest, ScriptRequestLanguage},
        };

        use crate::models::execution_process::{
            CreateExecutionProcess, ExecutionProcess, ExecutionProcessRunReason,
        };
        let request = CreateExecutionProcess {
            session_id: session,
            executor_action: ExecutorAction::new(
                ExecutorActionType::ScriptRequest(ScriptRequest {
                    script: "echo setup".into(),
                    language: ScriptRequestLanguage::Bash,
                    context: ScriptContext::SetupScript,
                    working_dir: None,
                }),
                None,
            ),
            run_reason: ExecutionProcessRunReason::SetupScript,
        };
        assert!(
            ExecutionProcess::create(&pool, &request, Uuid::new_v4(), &[])
                .await
                .unwrap_err()
                .to_string()
                .contains("reserved by formal Integration")
        );
        guard_workspace(&pool, workspace).await.unwrap_err();
        sqlx::query("DELETE FROM integration_reservations WHERE run_id=?")
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();
        guard_workspace(&pool, workspace).await.unwrap();
        sqlx::query("UPDATE agent_runs SET status='pending' WHERE id=?")
            .bind(agent)
            .execute(&pool)
            .await
            .unwrap();
        ExecutionProcess::create(&pool, &request, Uuid::new_v4(), &[])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn old_publication_never_completes_a_card_reserved_by_a_new_run() {
        let (pool, mut run, _) = fixture().await;
        run.payload.published = true;
        run.save(&pool).await.unwrap();
        let next = Uuid::new_v4();
        sqlx::query("INSERT INTO integration_runs(id,request_key,project_id,repository_id,storage_identity,target_ref,status,payload) SELECT ?,?,project_id,repository_id,storage_identity,target_ref,'queued',payload FROM integration_runs WHERE id=?")
            .bind(next).bind(next.to_string()).bind(run.id).execute(&pool).await.unwrap();
        sqlx::query("UPDATE integration_reservations SET run_id=? WHERE resource_kind='card'")
            .bind(next)
            .execute(&pool)
            .await
            .unwrap();
        let complete = IntegrationRun::complete_cards(&pool, run.id).await.unwrap();
        assert!(
            complete.payload.sources[0]
                .done_result
                .as_ref()
                .unwrap()
                .contains("another Integration")
        );
        let status: Uuid = sqlx::query_scalar("SELECT status_id FROM local_issues WHERE id=?")
            .bind(run.payload.sources[0].selection.card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, run.payload.sources[0].status_id);
    }

    #[tokio::test]
    async fn done_receipt_is_atomic_idempotent_and_never_undoes_reopening() {
        let (pool, mut run, done) = fixture().await;
        run.payload.published = true;
        run.save(&pool).await.unwrap();
        let completed = IntegrationRun::complete_cards(&pool, run.id).await.unwrap();
        let source = &completed.payload.sources[0];
        assert_eq!(source.done_result.as_deref(), Some("done"));
        let status: Uuid = sqlx::query_scalar("SELECT status_id FROM local_issues WHERE id=?")
            .bind(source.selection.card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, done);
        sqlx::query("UPDATE local_issues SET status_id=? WHERE id=?")
            .bind(source.status_id)
            .bind(source.selection.card_id)
            .execute(&pool)
            .await
            .unwrap();
        IntegrationRun::complete_cards(&pool, run.id).await.unwrap();
        let status: Uuid = sqlx::query_scalar("SELECT status_id FROM local_issues WHERE id=?")
            .bind(source.selection.card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, source.status_id);
    }

    #[tokio::test]
    async fn changed_requirements_after_publication_do_not_get_marked_done() {
        let (pool, mut run, _) = fixture().await;
        run.payload.published = true;
        run.save(&pool).await.unwrap();
        sqlx::query("DELETE FROM integration_reservations WHERE resource_kind='card'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE local_issues SET description='new request' WHERE id=?")
            .bind(run.payload.sources[0].selection.card_id)
            .execute(&pool)
            .await
            .unwrap();
        let completed = IntegrationRun::complete_cards(&pool, run.id).await.unwrap();
        assert!(
            completed.payload.sources[0]
                .done_result
                .as_deref()
                .unwrap()
                .starts_with("not changed")
        );
        let fk: Vec<sqlx::sqlite::SqliteRow> = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(fk.is_empty());
    }

    #[tokio::test]
    async fn relink_and_relink_back_after_publication_do_not_complete_either_card() {
        for restore_original_link in [false, true] {
            let (pool, mut run, _) = fixture().await;
            run.payload.published = true;
            run.save(&pool).await.unwrap();
            let source = &run.payload.sources[0];
            sqlx::query("DELETE FROM integration_reservations WHERE run_id=?")
                .bind(run.id)
                .execute(&pool)
                .await
                .unwrap();
            let other_card = Uuid::new_v4();
            sqlx::query("INSERT INTO local_issues(id,project_id,issue_number,simple_id,status_id,title,sort_order) VALUES(?,?,2,'TEST-2',?,'new task',1)")
                .bind(other_card).bind(run.project_id).bind(source.status_id)
                .execute(&pool).await.unwrap();
            sqlx::query("UPDATE local_workspace_links SET issue_id=? WHERE workspace_id=?")
                .bind(other_card)
                .bind(source.selection.workspace_id)
                .execute(&pool)
                .await
                .unwrap();
            if restore_original_link {
                sqlx::query("UPDATE local_workspace_links SET issue_id=? WHERE workspace_id=?")
                    .bind(source.selection.card_id)
                    .bind(source.selection.workspace_id)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            let completed = IntegrationRun::complete_cards(&pool, run.id).await.unwrap();
            assert!(
                completed.payload.sources[0]
                    .done_result
                    .as_deref()
                    .unwrap()
                    .starts_with("not changed")
            );
            for card in [source.selection.card_id, other_card] {
                let status: Uuid =
                    sqlx::query_scalar("SELECT status_id FROM local_issues WHERE id=?")
                        .bind(card)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert_eq!(status, source.status_id);
            }
            assert!(
                IntegrationRun::find(&pool, run.id)
                    .await
                    .unwrap()
                    .payload
                    .published
            );
        }
    }

    #[tokio::test]
    async fn cancel_and_publication_have_one_durable_winner() {
        let (pool, mut run, _) = fixture().await;
        run.status = "validating".into();
        run.save(&pool).await.unwrap();
        assert!(
            IntegrationRun::request_cancel(&pool, run.id)
                .await
                .unwrap()
                .cancel_requested
        );
        run.payload.publication_intent = true;
        let mut tx = pool.begin().await.unwrap();
        assert!(
            IntegrationRun::record_publication_intent(&mut tx, &run)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
        assert!(
            !IntegrationRun::find(&pool, run.id)
                .await
                .unwrap()
                .payload
                .publication_intent
        );

        let (pool, mut run, _) = fixture().await;
        run.status = "validating".into();
        run.save(&pool).await.unwrap();
        run.payload.publication_intent = true;
        let mut tx = pool.begin().await.unwrap();
        IntegrationRun::record_publication_intent(&mut tx, &run)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let late = IntegrationRun::request_cancel(&pool, run.id).await.unwrap();
        assert!(!late.cancel_requested);
        assert_eq!(late.status, "publishing");
        assert!(late.payload.publication_intent);
    }

    #[tokio::test]
    async fn requirements_change_and_revert_still_invalidates_done() {
        let (pool, mut run, _) = fixture().await;
        run.payload.published = true;
        run.save(&pool).await.unwrap();
        sqlx::query("DELETE FROM integration_reservations WHERE resource_kind='card'")
            .execute(&pool)
            .await
            .unwrap();
        let card = run.payload.sources[0].selection.card_id;
        for title in ["new requirement", "request"] {
            sqlx::query("UPDATE local_issues SET title=? WHERE id=?")
                .bind(title)
                .bind(card)
                .execute(&pool)
                .await
                .unwrap();
        }
        let result = IntegrationRun::complete_cards(&pool, run.id).await.unwrap();
        assert!(
            result.payload.sources[0]
                .done_result
                .as_deref()
                .unwrap()
                .starts_with("not changed")
        );
    }

    #[tokio::test]
    async fn done_and_receipt_roll_back_together_on_database_failure() {
        let (pool, mut run, _) = fixture().await;
        run.payload.published = true;
        run.save(&pool).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_integration_receipt BEFORE UPDATE OF payload ON integration_runs BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").execute(&pool).await.unwrap();
        assert!(IntegrationRun::complete_cards(&pool, run.id).await.is_err());
        let status: Uuid = sqlx::query_scalar("SELECT status_id FROM local_issues WHERE id=?")
            .bind(run.payload.sources[0].selection.card_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, run.payload.sources[0].status_id);
        assert!(
            IntegrationRun::find(&pool, run.id)
                .await
                .unwrap()
                .payload
                .sources[0]
                .done_result
                .is_none()
        );
        sqlx::query("DROP TRIGGER fail_integration_receipt")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            IntegrationRun::complete_cards(&pool, run.id)
                .await
                .unwrap()
                .payload
                .sources[0]
                .done_result
                .as_deref(),
            Some("done")
        );
    }

    #[tokio::test]
    async fn startup_fence_prevents_redelivery_and_retains_business_reservations() {
        let (pool, mut run, _) = fixture().await;
        let ws = run.payload.sources[0].selection.workspace_id;
        let session: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE workspace_id=?")
            .bind(ws)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            guard_agent_dispatch(&pool, ws, session, Uuid::new_v4())
                .await
                .is_err()
        );
        run.workspace_id = Some(ws);
        run.session_id = Some(session);
        run.status = "preparing".into();
        run.save(&pool).await.unwrap();
        guard_agent_dispatch(&pool, ws, session, run.id)
            .await
            .unwrap();
        assert!(
            guard_agent_dispatch(&pool, ws, Uuid::new_v4(), run.id)
                .await
                .is_err()
        );
        IntegrationRun::fence_interrupted_preparations(&pool)
            .await
            .unwrap();
        assert!(
            guard_agent_dispatch(&pool, ws, session, run.id)
                .await
                .is_err()
        );
        assert_eq!(workspace_owner(&pool, ws).await.unwrap(), Some(run.id));
        let fenced = IntegrationRun::find(&pool, run.id).await.unwrap();
        assert_eq!(fenced.status, "cancelling");
        assert!(fenced.error.unwrap().contains("restart"));
        sqlx::query("DELETE FROM integration_reservations WHERE run_id=?")
            .bind(run.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            guard_agent_dispatch(&pool, ws, session, run.id)
                .await
                .is_err()
        );
    }
}
