use axum::{Router, response::Json, routing::get};
use services::services::pipelines::{self, Pipeline};
use utils::{path::pipelines_dir, response::ApiResponse};

use crate::DeploymentImpl;

pub fn router() -> Router<DeploymentImpl> {
    Router::new().route("/pipelines", get(list))
}

async fn list() -> Json<ApiResponse<Vec<Pipeline>>> {
    Json(ApiResponse::success(pipelines::load_pipelines(
        &pipelines_dir(),
    )))
}
