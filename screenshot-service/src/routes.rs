use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::{config::AppState, error::Error, s3};

#[derive(Serialize, Deserialize)]
pub struct ImageUploadRequest {
    image: String,
    exam_attempt_id: String,
}

#[instrument(skip_all, err(Debug))]
pub async fn post_upload(
    State(state): State<AppState>,
    Json(image_upload_request): Json<ImageUploadRequest>,
) -> Result<impl IntoResponse, Error> {
    let image = image_upload_request.image;
    let exam_attempt_id = image_upload_request.exam_attempt_id;

    s3::upload_to_s3(
        &state.client,
        &state.env_vars.bucket_name,
        image,
        &exam_attempt_id,
    )
    .await
}

pub async fn get_status_ping() -> impl IntoResponse {
    info!("Status");
    StatusCode::OK
}
