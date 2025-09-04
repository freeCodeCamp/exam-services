use std::time::Duration;

use moderation_service::{config::EnvVars, db::update_moderation_collection};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(sentry::integrations::tracing::layer())
        .with(EnvFilter::from_default_env())
        .init();
    info!("Starting exam moderation service...");
    dotenvy::dotenv().ok();

    let env_vars = EnvVars::new();

    let _guard = if let Some(sentry_dsn) = env_vars.sentry_dsn.clone() {
        tracing::info!("initializing Sentry");
        // NOTE: Events are only emitted, once the guard goes out of scope.
        Some(sentry::init((
            sentry_dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                environment: Some(env_vars.environment.to_string().into()),
                traces_sample_rate: 1.0,
                ..Default::default()
            },
        )))
    } else {
        None
    };

    let task = update_moderation_collection(&env_vars);

    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
        info!("Received SIGINT (Ctrl+C), starting graceful shutdown...");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
        info!("Received SIGTERM, starting graceful shutdown...");
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    let task = async move {
        if let Some(secs) = env_vars.timeout_secs {
            match tokio::time::timeout(Duration::from_secs(secs), task).await {
                Ok(task_result) => match task_result {
                    Ok(_) => {}
                    Err(e) => {
                        error!("{e}");
                    }
                },
                Err(_) => {
                    error!("Task timed out after {secs} seconds");
                }
            }
        } else {
            if let Err(e) = task.await {
                error!("{e}");
            }
            info!("Task completed - exiting.");
        }
    };

    tokio::select! {
        _ = task => { },
        _ = ctrl_c => {
            // Migration future dropped here (cancelled)
        },
        _ = terminate => {
            // Migration future dropped here (cancelled)
        },
    };
}

// Tests are needed for schema changes
#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;
    use moderation_service::db;
    use mongodb::bson::doc;
    use prisma::{ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentExamModeration};

    fn setup() {
        dotenvy::dotenv().ok();
    }

    /// Check if all records in the `EnvExam` collection are deserializable
    #[tokio::test]
    async fn exam_schema_is_unchanged() {
        setup();
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
        setup();
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
        setup();
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
