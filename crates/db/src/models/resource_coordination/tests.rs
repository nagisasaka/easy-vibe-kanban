use super::*;

async fn setup() -> (SqlitePool, Uuid) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    fixture(pool).await
}

async fn fixture(pool: SqlitePool) -> (SqlitePool, Uuid) {
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,branch) VALUES(?,'test')")
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
    (pool, session)
}
async fn resource(pool: &SqlitePool, key: &str) -> SharedResource {
    SharedResource::register(
        pool,
        RegisterResource {
            resource_key: key.into(),
            name: key.into(),
            description: "Exclusive test resource; verify no active writer".into(),
            state: "schema-v1".into(),
        },
    )
    .await
    .unwrap()
}
fn spec(session: Uuid, resources: &[&SharedResource]) -> ResourceOperationSpec {
    ResourceOperationSpec {
        request_id: Uuid::new_v4(),
        session_id: session,
        purpose: "Test shared resource".into(),
        claims: resources
            .iter()
            .map(|r| ResourceClaim {
                resource_id: r.id,
                expected_revision: r.revision,
                resulting_state: None,
            })
            .collect(),
        script: "printf test".into(),
        verification_script: "test ! -e busy".into(),
        working_dir: ".".into(),
        timeout_seconds: 30,
    }
}
#[tokio::test]
async fn uncontended_requests_never_start_automatic_inference() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "one").await;
    let b = resource(&pool, "two").await;
    let one = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    ResourceOperation::submit(&pool, spec(s, &[&b]))
        .await
        .unwrap();
    assert!(
        ResourceMediation::request(&pool, one.id, false)
            .await
            .unwrap()
            .is_none()
    );
    ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    assert!(
        ResourceMediation::request(&pool, one.id, false)
            .await
            .unwrap()
            .is_some()
    );
}
#[tokio::test]
async fn bundles_are_atomic_fifo_and_disjoint_work_can_progress() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "phone").await;
    let b = resource(&pool, "db").await;
    let c = resource(&pool, "other").await;
    let first = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let runtime = Uuid::new_v4();
    assert_eq!(
        ResourceOperation::allocate(&pool, runtime).await.unwrap(),
        vec![first.id]
    );
    let bundle = ResourceOperation::submit(&pool, spec(s, &[&a, &b]))
        .await
        .unwrap();
    let later = ResourceOperation::submit(&pool, spec(s, &[&b]))
        .await
        .unwrap();
    let separate = ResourceOperation::submit(&pool, spec(s, &[&c]))
        .await
        .unwrap();
    assert_eq!(
        ResourceOperation::allocate(&pool, runtime).await.unwrap(),
        vec![separate.id]
    );
    let view = snapshot(&pool).await.unwrap();
    assert_eq!(view.holders.len(), 2);
    assert!(!view.holders.iter().any(|h| h.resource_id == b.id));
    ResourceOperation::finish(&pool, first.id, runtime, true, "verified")
        .await
        .unwrap();
    assert_eq!(
        ResourceOperation::allocate(&pool, runtime).await.unwrap(),
        vec![bundle.id]
    );
    assert_eq!(
        ResourceOperation::find(&pool, later.id)
            .await
            .unwrap()
            .status,
        "queued"
    );
}
#[tokio::test]
async fn concurrent_dispatchers_never_double_grant_and_retries_do_not_repeat() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(file.path())
        .busy_timeout(std::time::Duration::from_secs(10));
    let (pool, s) = fixture(SqlitePool::connect_with(options.clone()).await.unwrap()).await;
    let second_pool = SqlitePool::connect_with(options).await.unwrap();
    let a = resource(&pool, "phone").await;
    let request = spec(s, &[&a]);
    let first = ResourceOperation::submit(&pool, request.clone())
        .await
        .unwrap();
    assert_eq!(
        ResourceOperation::submit(&pool, request.clone())
            .await
            .unwrap()
            .id,
        first.id
    );
    let mut changed = request;
    changed.script = "different".into();
    assert!(ResourceOperation::submit(&pool, changed).await.is_err());
    ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let (one, two) = tokio::join!(
        ResourceOperation::allocate(&pool, Uuid::new_v4()),
        ResourceOperation::allocate(&second_pool, Uuid::new_v4())
    );
    assert_eq!(one.unwrap().len() + two.unwrap().len(), 1);
    assert_eq!(snapshot(&pool).await.unwrap().holders.len(), 1);
}
#[tokio::test]
async fn schema_change_blocks_old_assumptions_and_is_durable() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "database").await;
    let mut upgrade = spec(s, &[&a]);
    upgrade.claims[0].resulting_state = Some("schema-v2".into());
    let first = ResourceOperation::submit(&pool, upgrade).await.unwrap();
    let next = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let runtime = Uuid::new_v4();
    ResourceOperation::allocate(&pool, runtime).await.unwrap();
    ResourceOperation::finish(&pool, first.id, runtime, true, "verified schema")
        .await
        .unwrap();
    assert!(
        ResourceOperation::allocate(&pool, runtime)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ResourceOperation::find(&pool, next.id)
            .await
            .unwrap()
            .status,
        "blocked"
    );
    let view = snapshot(&pool).await.unwrap();
    assert_eq!(view.resources[0].revision, 2);
    assert_eq!(view.resources[0].state, "schema-v2");
    assert!(view.holders.is_empty());
    assert!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM resource_events WHERE kind='state_changed'"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
            > 0
    );
}
#[tokio::test]
async fn cancellation_or_timeout_never_releases_an_owner_and_old_fences_fail() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "phone").await;
    let first = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let runtime = Uuid::new_v4();
    ResourceOperation::allocate(&pool, runtime).await.unwrap();
    ResourceOperation::cancel(&pool, first.id).await.unwrap();
    assert_eq!(snapshot(&pool).await.unwrap().holders.len(), 1);
    ResourceOperation::finish(
        &pool,
        first.id,
        runtime,
        true,
        "late success after cancellation",
    )
    .await
    .unwrap();
    let view = snapshot(&pool).await.unwrap();
    assert_eq!(view.resources[0].health, "recovery_required");
    assert_eq!(view.holders.len(), 1);
    assert!(
        ResourceOperation::finish(&pool, first.id, runtime, true, "duplicate result")
            .await
            .is_err()
    );
    assert!(
        ResourceOperation::recover(
            &pool,
            first.id,
            ResourceRecovery {
                evidence: "checked".into(),
                claims: vec![]
            }
        )
        .await
        .is_err()
    );
    ResourceOperation::recover(
        &pool,
        first.id,
        ResourceRecovery {
            evidence: "Mock process terminated, lock file absent, schema checked".into(),
            claims: vec![ResourceClaim {
                resource_id: a.id,
                expected_revision: 1,
                resulting_state: Some("schema-v1".into()),
            }],
        },
    )
    .await
    .unwrap();
    let fresh = snapshot(&pool).await.unwrap().resources.remove(0);
    let second = ResourceOperation::submit(&pool, spec(s, &[&fresh]))
        .await
        .unwrap();
    ResourceOperation::allocate(&pool, runtime).await.unwrap();
    assert_eq!(snapshot(&pool).await.unwrap().holders[0].fence, 2);
    assert!(
        ResourceOperation::finish(&pool, first.id, runtime, true, "old token")
            .await
            .is_err()
    );
    assert_eq!(
        snapshot(&pool).await.unwrap().holders[0].operation_id,
        second.id
    );
}
#[tokio::test]
async fn failed_or_interrupted_commands_remain_reserved_across_new_dispatcher() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "aws").await;
    let first = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let old = Uuid::new_v4();
    ResourceOperation::allocate(&pool, old).await.unwrap();
    let next = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    assert!(
        ResourceOperation::allocate(&pool, Uuid::new_v4())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ResourceOperation::finish(&pool, first.id, Uuid::new_v4(), true, "wrong host")
            .await
            .is_err()
    );
    ResourceOperation::finish(&pool, first.id, old, false, "restart uncertainty")
        .await
        .unwrap();
    assert!(
        ResourceOperation::allocate(&pool, Uuid::new_v4())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ResourceOperation::find(&pool, next.id)
            .await
            .unwrap()
            .status,
        "queued"
    );
}
#[tokio::test]
async fn mediator_cannot_touch_active_owners_or_apply_stale_decisions() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "device").await;
    let owner = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let runtime = Uuid::new_v4();
    ResourceOperation::allocate(&pool, runtime).await.unwrap();
    let waiter = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    let m = ResourceMediation::request(&pool, waiter.id, false)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE resource_mediations SET status='running' WHERE id=?")
        .bind(m.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        ResourceMediation::apply(
            &pool,
            m.id,
            ResourceDecision {
                explanation: "invented order".into(),
                order: vec![owner.id, waiter.id],
                needs_replan: vec![]
            }
        )
        .await
        .is_err()
    );
    ResourceOperation::finish(&pool, owner.id, runtime, true, "done")
        .await
        .unwrap();
    assert!(
        !ResourceMediation::apply(
            &pool,
            m.id,
            ResourceDecision {
                explanation: "was valid but is stale".into(),
                order: vec![waiter.id],
                needs_replan: vec![]
            }
        )
        .await
        .unwrap()
    );
    assert!(
        ResourceMediation::request(&pool, waiter.id, false)
            .await
            .unwrap()
            .is_none()
    );
    let fresh = ResourceMediation::request(&pool, waiter.id, true)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE resource_mediations SET status='running' WHERE id=?")
        .bind(fresh.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        ResourceMediation::apply(
            &pool,
            fresh.id,
            ResourceDecision {
                explanation: "requires author clarification".into(),
                order: vec![waiter.id],
                needs_replan: vec![waiter.id]
            }
        )
        .await
        .unwrap()
    );
    assert_eq!(
        ResourceOperation::find(&pool, waiter.id)
            .await
            .unwrap()
            .status,
        "blocked"
    );
}
#[tokio::test]
async fn canonical_identity_duplicate_claims_and_workspace_cleanup_are_guarded() {
    let (pool, s) = setup().await;
    let a = resource(&pool, "serial-123").await;
    assert!(
        SharedResource::register(
            &pool,
            RegisterResource {
                resource_key: a.resource_key.clone(),
                name: "alias".into(),
                description: "duplicate".into(),
                state: "v1".into()
            }
        )
        .await
        .is_err()
    );
    assert!(
        ResourceOperation::submit(&pool, spec(s, &[&a, &a]))
            .await
            .is_err()
    );
    let op = ResourceOperation::submit(&pool, spec(s, &[&a]))
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE workspaces SET archived=1 WHERE id=?")
            .bind(op.workspace_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM workspaces WHERE id=?")
            .bind(op.workspace_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM sessions WHERE id=?")
            .bind(s)
            .execute(&pool)
            .await
            .is_err()
    );
    ResourceOperation::cancel(&pool, op.id).await.unwrap();
    sqlx::query("UPDATE workspaces SET archived=1 WHERE id=?")
        .bind(op.workspace_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sessions WHERE id=?")
        .bind(s)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=?")
        .bind(op.workspace_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        ResourceOperation::find(&pool, op.id).await.unwrap().status,
        "cancelled"
    );
}
