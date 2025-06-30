use aws_config::BehaviorVersion;
use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Clone)]
struct AppState {
    client: aws_sdk_s3::Client,
    env_vars: EnvVars,
}

#[derive(Debug, Clone)]
struct EnvVars {
    bucket_name: String,
    port: u16,
}

impl EnvVars {
    pub fn new() -> Self {
        let bucket_name = std::env::var("S3_BUCKET_NAME").unwrap();
        let port = match std::env::var("PORT") {
            Ok(port_string) => port_string.parse().expect("PORT to be parseable as u16"),
            Err(_e) => {
                let default_port = 3002;
                warn!("PORT not set. Defaulting to {default_port}");
                default_port
            }
        };
        EnvVars { bucket_name, port }
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        // Log to stdout
        .with(tracing_subscriber::fmt::layer().pretty())
        .init();

    info!("Starting server...");
    // TODO: Consider using a specific version
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_s3::Client::new(&config);
    let env_vars = EnvVars::new();
    let port = env_vars.port;
    let app_state = AppState { client, env_vars };

    let app = Router::new()
        .route("/status/ping", get(get_status_ping))
        .route("/upload", put(put_upload))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    let server = axum::serve(listener, app);

    // Create shutdown signal handler
    let shutdown_signal = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                info!("Received SIGINT (Ctrl+C), starting graceful shutdown...");
            },
            _ = terminate => {
                info!("Received SIGTERM, starting graceful shutdown...");
            },
        }
    };

    // Run server with graceful shutdown
    if let Err(err) = server.with_graceful_shutdown(shutdown_signal).await {
        error!("Server error: {}", err);
    }
}

async fn get_status_ping() {}

#[derive(Serialize, Deserialize)]
struct ImageUploadRequest {
    image: String,
    exam_attempt_id: String,
}

async fn put_upload(
    State(state): State<AppState>,
    Json(image_upload_request): Json<ImageUploadRequest>,
) {
    let image = image_upload_request.image;
    let exam_attempt_id = image_upload_request.exam_attempt_id;

    upload_to_s3(
        &state.client,
        &state.env_vars.bucket_name,
        image,
        &exam_attempt_id,
    )
    .await;
    todo!()
}

async fn upload_to_s3(
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
