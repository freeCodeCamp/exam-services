use std::time::Duration;

use aws_config::BehaviorVersion;
use axum::{
    Router,
    routing::{get, post},
};
use tokio::signal;
use tower_http::{
    LatencyUnit,
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod error;
mod routes;
mod s3;

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

    let env_vars = config::EnvVars::new();
    let port = env_vars.port;
    let request_timeout_in_ms = env_vars.request_timeout_in_ms;
    let request_body_size_limit = env_vars.request_body_size_limit;

    let app_state = config::AppState { client, env_vars };

    let app = Router::new()
        .route("/status/ping", get(routes::get_status_ping))
        .route("/upload", post(routes::post_upload))
        .layer(TimeoutLayer::new(Duration::from_secs(
            request_timeout_in_ms,
        )))
        .layer(RequestBodyLimitLayer::new(request_body_size_limit))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(LatencyUnit::Micros),
                ),
        )
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());

    if let Err(err) = server.await {
        error!("Server error: {}", err);
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
