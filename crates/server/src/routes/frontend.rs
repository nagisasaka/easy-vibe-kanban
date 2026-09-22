use axum::{
    Router,
    body::Body,
    http::HeaderValue,
    response::{IntoResponse, Response},
    routing::get,
};
use reqwest::{StatusCode, header};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../packages/local-web/dist"]
struct Assets;

pub(super) fn router(api_routes: Router) -> Router {
    Router::new()
        .route("/", get(serve_frontend_root))
        .route("/{*path}", get(serve_frontend))
        .nest("/api", api_routes)
}

pub(super) async fn serve_frontend(
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    // Nested misses can reach this wildcard. API errors must never become a
    // successful SPA document (or loop through the Vite API proxy).
    if uri.path() == "/api" || uri.path().starts_with("/api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = path.trim_start_matches('/');
    serve_file(path).await
}

pub(super) async fn serve_frontend_root() -> impl IntoResponse {
    serve_file("index.html").await
}

async fn serve_file(path: &str) -> Response {
    let file = Assets::get(path);

    match file {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();

            Response::builder()
                .status(StatusCode::OK)
                .header(
                    header::CONTENT_TYPE,
                    HeaderValue::from_str(mime.as_ref()).unwrap(),
                )
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            // For SPA routing, serve index.html for unknown routes
            if let Some(index) = Assets::get("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, HeaderValue::from_static("text/html"))
                    .body(Body::from(index.data.into_owned()))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn production_fallback_keeps_api_misses_out_of_spa() {
        let app = router(
            Router::new()
                .route("/health", get(super::super::health::health_check))
                .route(
                    "/hosts/{host}/known",
                    get(|| async { axum::Json("host reply") }),
                ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        for path in [
            "/api",
            "/api/",
            "/api/no-such-route",
            "/api/hosts/unknown/missing",
        ] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(
                !response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .is_some_and(|value| value.to_str().unwrap().contains("text/html"))
            );
            assert!(!response.headers().contains_key(header::LOCATION));
            assert!(
                !client
                    .post(format!("{base}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .is_success()
            );
        }
        for path in ["/api/health", "/api/hosts/test/known"] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(
                response.headers()[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .contains("application/json")
            );
        }
        let index =
            Assets::get("index.html").expect("dev frontend assets used by the production server");
        for path in [
            "/",
            "/workspaces/example",
            "/apiculture",
            "/hosts/test/workspaces/example",
        ] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response.bytes().await.unwrap().as_ref(),
                index.data.as_ref()
            );
        }
        let asset = Assets::iter()
            .find(|name| name.ends_with(".js") || name.ends_with(".css"))
            .expect("bundled asset");
        let response = client.get(format!("{base}/{asset}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.bytes().await.unwrap().as_ref(),
            Assets::get(&asset).unwrap().data.as_ref()
        );
        server.abort();
    }
}
