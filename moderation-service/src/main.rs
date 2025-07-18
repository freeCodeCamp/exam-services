use moderation_service::{config::EnvVars, db::update_moderation_collection};
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

    let env_vars = EnvVars::new();

    let _guard = if let Some(sentry_dsn) = env_vars.sentry_dsn.clone() {
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
        None
    };

    if let Err(e) = update_moderation_collection(&env_vars).await {
        tracing::error!("Error updating moderation collection: {:?}", e);
    } else {
        tracing::info!("Successfully updated moderation collection");
    }
}

// Tests are needed for schema changes
#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;
    use moderation_service::{
        db,
        prisma::{ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentExamModeration},
    };
    use mongodb::bson::doc;

    /// Check if all records in the `EnvExam` collection are deserializable
    #[tokio::test]
    async fn exam_schema_is_unchanged() {
        let mongo_uri = std::env::var("MONGODB_URI").unwrap();
        let client = db::client(&mongo_uri).await.unwrap();
        let exam_collection =
            db::get_collection::<ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;
        let _exams: Vec<ExamEnvironmentExam> = exam_collection
            .find(doc! {})
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
    }

    /// Check if all records in the `EnvExamEnvironmentExamAttempt` collection are deserializable
    #[tokio::test]
    async fn attempt_schema_is_unchanged() {
        let mongo_uri = std::env::var("MONGODB_URI").unwrap();
        let client = db::client(&mongo_uri).await.unwrap();
        let attempt_collection =
            db::get_collection::<ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt")
                .await;
        let _attempts: Vec<ExamEnvironmentExamAttempt> = attempt_collection
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
        let moderation_collection = db::get_collection::<ExamEnvironmentExamModeration>(
            &client,
            "ExamEnvironmentExamModeration",
        )
        .await;
        let _moderations: Vec<ExamEnvironmentExamModeration> = moderation_collection
            .find(doc! {})
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
    }
}
