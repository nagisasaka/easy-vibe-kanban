//! An optional host-owned maintenance gate. No deployment privileges or update
//! machinery live in the application; this only drains HTTP request handlers.
use std::{path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::Request,
    http::{Method, StatusCode},
    middleware::{Next, from_fn},
    response::IntoResponse,
    routing::get,
};
use tokio::sync::RwLock;

const READY: &str = "/api/maintenance/ready";

pub fn with_maintenance(app: Router) -> Router {
    with_gate(app, PathBuf::from("/run/lvk-maintenance/active"))
}

fn with_gate(app: Router, path: PathBuf) -> Router {
    let barrier = Arc::new(RwLock::new(()));
    let gate = Arc::new(path);
    let ready_barrier = barrier.clone();
    let ready_gate = gate.clone();
    app.route(
        READY,
        get(move || {
            let barrier = ready_barrier.clone();
            let gate = ready_gate.clone();
            async move {
                let _drained = barrier.write().await;
                if !gate.try_exists().unwrap_or(true) {
                    return StatusCode::CONFLICT.into_response();
                }
                (StatusCode::OK, [("x-lvk-maintenance-barrier", "1")]).into_response()
            }
        }),
    )
    .layer(from_fn(move |request: Request, next: Next| {
        let barrier = barrier.clone();
        let gate = gate.clone();
        async move {
            if matches!(*request.method(), Method::GET | Method::HEAD)
                && request.uri().path() == READY
            {
                return next.run(request).await;
            }
            let mutating = !matches!(
                *request.method(),
                Method::GET | Method::HEAD | Method::OPTIONS
            ) || request.headers().contains_key("upgrade");
            if mutating && gate.try_exists().unwrap_or(true) {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            let _request = barrier.read().await;
            // Close the race with a host gate written while admission waited.
            if mutating && gate.try_exists().unwrap_or(true) {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            next.run(request).await
        }
    }))
}

#[cfg(test)]
mod tests {
    use axum::routing::post;
    use tokio::sync::Notify;

    use super::*;

    #[tokio::test]
    async fn readiness_waits_for_older_mutations_and_rejects_new_writers() {
        let directory = tempfile::tempdir().unwrap();
        let gate = directory.path().join("active");
        let entered = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let handler_entered = entered.clone();
        let handler_finish = finish.clone();
        let app = with_gate(
            Router::new()
                .route("/read", get(|| async { StatusCode::OK }))
                .route(
                    "/write",
                    post(move || {
                        let entered = handler_entered.clone();
                        let finish = handler_finish.clone();
                        async move {
                            entered.notify_one();
                            finish.notified().await;
                            StatusCode::OK
                        }
                    }),
                ),
            gate.clone(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        // Axum routes HEAD through the GET handler too; neither may acquire a
        // read lock before the handler's exclusive readiness lock.
        assert_eq!(
            client
                .head(format!("{url}{READY}"))
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
                .unwrap()
                .status(),
            409
        );
        assert_eq!(
            client
                .get(format!("{url}{READY}"))
                .send()
                .await
                .unwrap()
                .status(),
            409
        );
        let writer = tokio::spawn(client.post(format!("{url}/write")).send());
        entered.notified().await;
        std::fs::write(&gate, "host-owned").unwrap();
        let mut ready = tokio::spawn(client.get(format!("{url}{READY}")).send());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut ready)
                .await
                .is_err()
        );
        assert_eq!(
            client
                .post(format!("{url}/write"))
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        finish.notify_one();
        assert_eq!(writer.await.unwrap().unwrap().status(), 200);
        let response = ready.await.unwrap().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-lvk-maintenance-barrier"], "1");
        assert_eq!(
            client
                .get(format!("{url}/read"))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            client
                .get(format!("{url}/read"))
                .header("upgrade", "websocket")
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        server.abort();
    }
}
