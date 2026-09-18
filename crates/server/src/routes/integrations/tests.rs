use std::path::Path;

use db::models::{
    integration::{IntegrationPayload, IntegrationRun},
    repo::Repo,
    workspace::{CreateWorkspace, Workspace},
    workspace_repo::{CreateWorkspaceRepo, WorkspaceRepo},
};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::{admission, project_repository_ids};

async fn project(pool: &SqlitePool) -> (Uuid, Uuid) {
    let id = Uuid::new_v4();
    let status = Uuid::new_v4();
    let card = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,name) VALUES(?,'local Board')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO local_project_statuses(id,project_id,name,color,sort_order) VALUES(?,?,'Todo','blue',0)")
        .bind(status).bind(id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO local_issues(id,project_id,issue_number,simple_id,status_id,title,sort_order) VALUES(?,?,1,'TEST-1',?,'request',0)")
        .bind(card).bind(id).bind(status).execute(pool).await.unwrap();
    (id, card)
}

#[tokio::test]
async fn local_board_repository_candidates_follow_real_workspace_links_without_legacy_binding() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
    let (project_a, card_a) = project(&pool).await;
    let (project_b, card_b) = project(&pool).await;
    let repo = Repo::find_or_create(&pool, Path::new("/local-board-repo"), "repo")
        .await
        .unwrap();
    let unrelated = Repo::find_or_create(&pool, Path::new("/unrelated-repo"), "other")
        .await
        .unwrap();
    let mut workspaces = Vec::new();
    for _ in 0..2 {
        let workspace = Workspace::create(
            &pool,
            &CreateWorkspace {
                branch: "feature".into(),
                name: None,
            },
            Uuid::new_v4(),
        )
        .await
        .unwrap();
        WorkspaceRepo::create_many(
            &pool,
            workspace.id,
            &[CreateWorkspaceRepo {
                repo_id: repo.id,
                target_branch: "main".into(),
            }],
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO local_workspace_links(workspace_id,issue_id,project_id) VALUES(?,?,?)",
        )
        .bind(workspace.id)
        .bind(card_a)
        .bind(project_a)
        .execute(&pool)
        .await
        .unwrap();
        workspaces.push(workspace.id);
    }
    let legacy_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_repos")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(legacy_count, 0);
    assert_eq!(
        project_repository_ids(&pool, project_a).await.unwrap(),
        vec![repo.id]
    );
    assert!(
        project_repository_ids(&pool, project_b)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !project_repository_ids(&pool, project_a)
            .await
            .unwrap()
            .contains(&unrelated.id)
    );
    // Re-linking changes current Board membership, not historical event ownership.
    for id in workspaces {
        sqlx::query(
            "UPDATE local_workspace_links SET issue_id=?,project_id=? WHERE workspace_id=?",
        )
        .bind(card_b)
        .bind(project_b)
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }
    assert!(
        project_repository_ids(&pool, project_a)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        project_repository_ids(&pool, project_b).await.unwrap(),
        vec![repo.id]
    );
    // Existing explicit bindings still work; overlapping associations deduplicate.
    for project in [project_a, project_b] {
        sqlx::query("INSERT INTO project_repos(id,project_id,repo_id) VALUES(?,?,?)")
            .bind(Uuid::new_v4())
            .bind(project)
            .bind(repo.id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            project_repository_ids(&pool, project).await.unwrap(),
            vec![repo.id]
        );
    }
}

#[tokio::test]
async fn target_queue_uses_storage_identity_and_keeps_recovery_reservations() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
    let (project, _) = project(&pool).await;
    let mut runs = Vec::new();
    for (index, storage) in ["/git/shared", "/git/shared", "/git/independent"]
        .iter()
        .enumerate()
    {
        // Different registered IDs must not bypass the common-storage FIFO.
        let repo =
            Repo::find_or_create(&pool, Path::new(&format!("/registration-{index}")), "repo")
                .await
                .unwrap();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO integration_runs(id,request_key,project_id,repository_id,storage_identity,target_ref,status,payload,created_at) VALUES(?,?,?,?,?,'refs/heads/main','queued',?,?)")
            .bind(id).bind(id.to_string()).bind(project).bind(repo.id).bind(storage)
            .bind(sqlx::types::Json(IntegrationPayload::default()))
            .bind(format!("2026-09-18 00:00:0{index}"))
            .execute(&pool).await.unwrap();
        runs.push(IntegrationRun::find(&pool, id).await.unwrap());
    }
    assert!(!admission::target_right(&pool, &runs[1]).await.unwrap());
    assert!(admission::target_right(&pool, &runs[0]).await.unwrap());
    assert!(admission::target_right(&pool, &runs[0]).await.unwrap());
    assert!(admission::target_right(&pool, &runs[2]).await.unwrap());
    sqlx::query("UPDATE integration_runs SET status='recovery_required' WHERE id=?")
        .bind(runs[0].id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!admission::target_right(&pool, &runs[1]).await.unwrap());
    // Neither missing dispatcher leases nor a terminal row alone releases the
    // business reservation. Only confirmed cleanup/recovery calls release.
    sqlx::query("DELETE FROM orchestration_leases")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE integration_runs SET status='cancelled' WHERE id=?")
        .bind(runs[0].id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!admission::target_right(&pool, &runs[1]).await.unwrap());
    admission::release(&pool, runs[0].id).await.unwrap();
    assert!(admission::target_right(&pool, &runs[1]).await.unwrap());
}
