use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use moderation_service::{
    config::EnvVars,
    db::{
        auto_approve_moderation_records, award_challenge_ids, delete_practice_exam_attempts,
        delete_supabase_events, update_moderation_collection,
    },
};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let sentry_layer =
        sentry::integrations::tracing::layer().event_filter(|md| match *md.level() {
            // Capture error level events as Sentry events
            // These are grouped into issues, representing high-severity errors to act upon
            tracing::Level::ERROR => {
                sentry::integrations::tracing::EventFilter::Event
                    | sentry::integrations::tracing::EventFilter::Log
            }
            // Ignore trace level events, as they're too verbose
            tracing::Level::TRACE => sentry::integrations::tracing::EventFilter::Ignore,
            // Capture everything else as a traditional structured log
            _ => sentry::integrations::tracing::EventFilter::Log,
        });

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=info", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(sentry_layer)
        .init();
    info!("Starting exam moderation service...");
    dotenvy::dotenv().ok();

    let env_vars = EnvVars::new();

    let _guard = if let Some(sentry_dsn) = env_vars.sentry_dsn.clone() {
        info!("initializing Sentry");
        // NOTE: Events are only emitted, once the guard goes out of scope.
        Some(sentry::init((
            sentry_dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                environment: Some(env_vars.environment.to_string().into()),
                traces_sample_rate: 1.0,
                enable_logs: true,
                ..Default::default()
            },
        )))
    } else {
        None
    };

    // Build a future that runs all registered tasks (easy to extend by adding to the vector
    // inside `run_registered_tasks`).
    let all_tasks = run_registered_tasks(&env_vars);

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

    let task = async {
        if let Some(secs) = env_vars.timeout_secs {
            match tokio::time::timeout(Duration::from_secs(secs), all_tasks).await {
                Ok(_) => info!("All tasks completed within timeout."),
                Err(_) => error!("Tasks timed out after {secs} seconds"),
            }
        } else {
            all_tasks.await;
            info!("All tasks completed - exiting.");
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

/// Runs all registered maintenance tasks synchronously. Order matters.
/// To add a new task, just push a (name, future) pair into the `tasks` vector.
async fn run_registered_tasks(env_vars: &EnvVars) {
    // Clone env vars so each task owns its copy (allowing 'static futures)
    type TaskFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

    let tasks: Vec<(&'static str, TaskFuture)> = vec![
        {
            // Delete practice exams attempts before other tasks to avoid unnecessary work
            let env = env_vars.clone();
            (
                "delete_practice_exam_attempts",
                Box::pin(async move { delete_practice_exam_attempts(&env).await }),
            )
        },
        {
            // Update the moderation collection to represent current state of attempts before tasks altering moderations
            let env = env_vars.clone();
            (
                "update_moderation_collection",
                Box::pin(async move { update_moderation_collection(&env).await }),
            )
        },
        {
            // Approve old-enough moderations
            let env = env_vars.clone();
            (
                "auto_approve_moderation_records",
                Box::pin(async move { auto_approve_moderation_records(&env).await }),
            )
        },
        {
            // Handle challenge ids after moderations have been completely updated
            let env = env_vars.clone();
            (
                "award_challenge_ids",
                Box::pin(async move { award_challenge_ids(&env).await }),
            )
        },
        {
            // Handle clean-up of old supabase events
            let env = env_vars.clone();
            (
                "delete_supabase_events",
                Box::pin(async move { delete_supabase_events(&env).await }),
            )
        },
    ];

    for (name, fut) in tasks {
        match fut.await {
            Ok(_) => info!("Task {name} completed"),
            Err(e) => error!("Task {name} failed: {e:?}"),
        }
    }
}

// Tests are needed for schema changes
#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;
    use mongodb::bson::doc;
    use prisma::{
        ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentExamModeration, db,
    };

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
