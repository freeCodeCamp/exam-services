use axum::http::StatusCode;
use tracing::info;

use crate::error::Error;

pub async fn upload_to_s3(
    client: &aws_sdk_s3::Client,
    bucket_name: &str,
    image: String,
    exam_attempt_id: &str,
) -> Result<(), Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let key = format!("{exam_attempt_id}/{now}");
    let put_object_output = client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .content_type("image/jpeg")
        .body(image.into_bytes().into())
        .send()
        .await
        .map_err(|e| Error::Server(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?;

    let expiry = put_object_output.expiration;

    info!("Object expiry: {expiry:?}");

    Ok(())
}
