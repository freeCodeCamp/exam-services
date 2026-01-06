use tracing::error;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod ensure_awarded_challenges;
mod util;
use ensure_awarded_challenges::ensure_awarded_challenges;
use util::client;

#[tokio::main]
async fn main() {
    let file = std::fs::File::create("logs.jsonl").expect("unable to create file");
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=info", env!("CARGO_CRATE_NAME")).into()),
        )
        // .with(tracing_subscriber::fmt::layer().pretty())
        .with(tracing_subscriber::fmt::layer().json().with_writer(file))
        .init();
    dotenvy::dotenv().ok();
    let mongo_uri = std::env::var("MONGODB_URI").unwrap();
    let client = client(&mongo_uri).await.unwrap();
    if let Err(e) = ensure_awarded_challenges(client).await {
        error!("{e:?}");
    }
}
