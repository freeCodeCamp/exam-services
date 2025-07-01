use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{config::AppState, s3};

#[derive(Serialize, Deserialize)]
pub struct ImageUploadRequest {
    image: String,
    exam_attempt_id: String,
}

pub async fn post_upload(
    State(state): State<AppState>,
    Json(image_upload_request): Json<ImageUploadRequest>,
) {
    let image = image_upload_request.image;
    let exam_attempt_id = image_upload_request.exam_attempt_id;

    s3::upload_to_s3(
        &state.client,
        &state.env_vars.bucket_name,
        image,
        &exam_attempt_id,
    )
    .await;
    todo!()
}

pub async fn get_status_ping() -> impl IntoResponse {
    info!("Status");
    StatusCode::OK
}
