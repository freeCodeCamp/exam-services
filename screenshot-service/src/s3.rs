use axum::response::IntoResponse;

pub async fn upload_to_s3(
    client: &aws_sdk_s3::Client,
    bucket_name: &str,
    image: String,
    exam_attempt_id: &str,
) -> impl IntoResponse {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let key = format!("{exam_attempt_id}/{now}");
    let res = client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(image.into_bytes().into())
        .send()
        .await;

    match res {
        Ok(_) => Ok(()),
        Err(e) => Err(()),
    }
}
