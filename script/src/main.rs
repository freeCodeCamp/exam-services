use tracing::error;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod award_challenges_from_date;
mod util;
use award_challenges_from_date::award_challenges_from_date;
use util::client;

#[tokio::main]
async fn main() {
    let file = std::fs::File::create("logs.jsonl").expect("unable to create file");
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=info", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(tracing_subscriber::fmt::layer().json().with_writer(file))
        .init();
    dotenvy::dotenv().ok();
    let mongo_uri = std::env::var("MONGODB_URI").unwrap();
    let client = client(&mongo_uri).await.unwrap();
    if let Err(e) = award_challenges_from_date(client).await {
        error!("{e:?}");
    }
}
