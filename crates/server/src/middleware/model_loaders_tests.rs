use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Router, middleware::from_fn_with_state, routing::any};
use db::models::{
    session::CreateSession, workspace::CreateWorkspace, workspace_usage::WorkspaceExecutionOwner,
};

use super::*;

#[tokio::test]
async fn public_workspace_and_session_routes_reject_before_handler_side_effects() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("../db/migrations").run(&pool).await.unwrap();
    let ordinary = Workspace::create(
        &pool,
        &CreateWorkspace {
            name: None,
            branch: "normal".into(),
        },
        Uuid::new_v4(),
    )
    .await
    .unwrap();
    let internal = Workspace::create_with_owner(
        &pool,
        &CreateWorkspace {
            name: None,
            branch: "execution".into(),
        },
        Uuid::new_v4(),
        Some(&WorkspaceExecutionOwner::new(
            "future_owner",
            Uuid::new_v4(),
            None,
        )),
    )
    .await
    .unwrap();
    let session = Session::create(
        &pool,
        &CreateSession {
            name: None,
            executor: None,
        },
        Uuid::new_v4(),
        internal.id,
    )
    .await
    .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let handler = {
        let effects = effects.clone();
        move || {
            let effects = effects.clone();
            async move {
                effects.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }
        }
    };
    let workspace_paths = [
        "",
        "/git/merge",
        "/git/rebase",
        "/git/push",
        "/repos",
        "/execution/start",
        "/execution/setup",
        "/execution/cleanup",
        "/integration/open-editor",
        "/integration/editor/path",
        "/attachments",
        "/links",
        "/wiki/language",
        "/files/content",
    ];
    let mut workspace_routes = Router::new();
    for suffix in workspace_paths
        .into_iter()
        .chain(["/files/tree", "/seen", "/execution/stop"])
    {
        workspace_routes =
            workspace_routes.route(&format!("/workspaces/{{id}}{suffix}"), any(handler.clone()));
    }
    let mut session_routes = Router::new();
    for suffix in ["", "/follow-up", "/queue", "/setup", "/review"] {
        session_routes =
            session_routes.route(&format!("/sessions/{{id}}{suffix}"), any(handler.clone()));
    }
    let router = Router::new()
        .merge(workspace_routes.layer(from_fn_with_state(pool.clone(), load_workspace_from_pool)))
        .merge(session_routes.layer(from_fn_with_state(pool.clone(), load_session_from_pool)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    for suffix in workspace_paths {
        for method in [
            reqwest::Method::POST,
            reqwest::Method::PUT,
            reqwest::Method::DELETE,
        ] {
            let response = client.request(method, format!("http://{address}/workspaces/{}{suffix}", internal.id))
                .json(&serde_json::json!({ "internal": true, "owner_id": internal.execution_owner, "usage": "interactive" })).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::CONFLICT, "{suffix}");
        }
    }
    for suffix in ["", "/follow-up", "/queue", "/setup", "/review"] {
        let response = client
            .post(format!("http://{address}/sessions/{}{suffix}", session.id))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{suffix}");
    }
    assert_eq!(
        effects.load(Ordering::SeqCst),
        0,
        "Rejection must precede every downstream effect"
    );
    assert_eq!(
        client
            .get(format!(
                "http://{address}/workspaces/{}/integration/editor/path",
                internal.id
            ))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        effects.load(Ordering::SeqCst),
        0,
        "Relay editor admission is not passive file inspection"
    );
    assert!(
        db::models::workspace_usage::require_interactive(&pool, internal.id)
            .await
            .is_err()
    );
    for (method, path) in [
        (
            reqwest::Method::GET,
            format!("workspaces/{}/files/tree", internal.id),
        ),
        (reqwest::Method::GET, format!("sessions/{}", session.id)),
        (
            reqwest::Method::PUT,
            format!("workspaces/{}/seen", internal.id),
        ),
        // This route is admitted only to the owner-specific controller, not a freeform dispatch.
        (
            reqwest::Method::POST,
            format!("workspaces/{}/execution/stop", internal.id),
        ),
        (
            reqwest::Method::POST,
            format!("workspaces/{}/git/merge", ordinary.id),
        ),
    ] {
        assert_eq!(
            client
                .request(method, format!("http://{address}/{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(effects.load(Ordering::SeqCst), 5);
    assert_eq!(
        Session::find_by_workspace_id(&pool, internal.id)
            .await
            .unwrap()
            .len(),
        1
    );
    for table in ["agent_runs", "execution_processes", "local_workspace_links"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
    }
    server.abort();
}
