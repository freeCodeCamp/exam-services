use moderation_service::db::update_moderation_collection;
use sentry::types::Dsn;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(sentry::integrations::tracing::layer())
        .with(EnvFilter::from_default_env())
        .init();
    tracing::info!("Starting exam moderation service...");
    dotenvy::dotenv().ok();

    let sentry_dsn = std::env::var("SENTRY_DSN").unwrap_or_default();
    let _guard = if valid_sentry_dsn(&sentry_dsn) {
        tracing::info!("initializing Sentry");
        // NOTE: Events are only emitted, once the guard goes out of scope.
        Some(sentry::init((
            sentry_dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                traces_sample_rate: 1.0,
                ..Default::default()
            },
        )))
    } else {
        tracing::warn!("Sentry DSN is invalid. skipping initialization");
        None
    };

    if let Err(e) = update_moderation_collection().await {
        tracing::error!("Error updating moderation collection: {:?}", e);
    } else {
        tracing::info!("Successfully updated moderation collection");
    }
}

pub fn valid_sentry_dsn(url: &str) -> bool {
    url.parse::<Dsn>().is_ok()
}

// Tests are needed for schema changes
#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;
    use moderation_service::{
        db,
        prisma::{EnvExam, EnvExamAttempt, ExamModeration},
    };
    use mongodb::bson::doc;

    /// Check if all records in the `EnvExam` collection are deserializable
    #[tokio::test]
    async fn exam_schema_is_unchanged() {
        let mongo_uri = std::env::var("MONGODB_URI").unwrap();
        let client = db::client(&mongo_uri).await.unwrap();
        let exam_collection = db::get_collection::<EnvExam>(&client, "EnvExam").await;
        let _exams: Vec<EnvExam> = exam_collection
            .find(doc! {})
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
    }

    /// Check if all records in the `EnvExamAttempt` collection are deserializable
    #[tokio::test]
    async fn attempt_schema_is_unchanged() {
        let mongo_uri = std::env::var("MONGODB_URI").unwrap();
        let client = db::client(&mongo_uri).await.unwrap();
        let attempt_collection =
            db::get_collection::<EnvExamAttempt>(&client, "EnvExamAttempt").await;
        let _attempts: Vec<EnvExamAttempt> = attempt_collection
            .find(doc! {})
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
    }

    /// Check if all records in the `ExamModeration` collection are deserializable
    #[tokio::test]
    async fn moderation_schema_is_unchanged() {
        let mongo_uri = std::env::var("MONGODB_URI").unwrap();
        let client = db::client(&mongo_uri).await.unwrap();
        let moderation_collection =
            db::get_collection::<ExamModeration>(&client, "ExamModeration").await;
        let _moderations: Vec<ExamModeration> = moderation_collection
            .find(doc! {})
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
    }
}
