use chrono::TimeZone;
use futures_util::{StreamExt, TryStreamExt};
use mongodb::{
    Client, Collection,
    bson::{doc, oid::ObjectId},
    options::ClientOptions,
};
use serde::{Deserialize, Serialize};

use prisma::{
    ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentExamModeration,
    ExamEnvironmentExamModerationStatus,
};

use crate::config::EnvVars;

pub async fn get_collection<'d, T>(client: &Client, collection_name: &str) -> Collection<T>
where
    T: Send + Sync + Deserialize<'d> + Serialize,
{
    let db = client.database("freecodecamp");

    let collection = db.collection::<T>(collection_name);
    collection
}

pub async fn client(uri: &str) -> mongodb::error::Result<Client> {
    let mut client_options = ClientOptions::parse(uri).await?;

    client_options.app_name = Some("exam-moderation-service".to_string());

    // Get a handle to the cluster
    let client = Client::with_options(client_options)?;

    // Ping the server to see if you can connect to the cluster
    client
        .database("freecodecamp")
        .run_command(doc! {"ping": 1})
        .await?;

    Ok(client)
}

#[tracing::instrument]
pub async fn update_moderation_collection(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let moderation_collection =
        get_collection::<ExamEnvironmentExamModeration>(&client, "ExamEnvironmentExamModeration")
            .await;
    let attempt_collection =
        get_collection::<ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt").await;
    let exam_collection =
        get_collection::<ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;

    let moderation_records: Vec<ExamEnvironmentExamModeration> = moderation_collection
        .find(doc! {})
        .projection(doc! {"examAttemptId": true})
        .await?
        .try_collect()
        .await?;
    let exam_attempt_ids: Vec<ObjectId> = moderation_records
        .iter()
        .map(|r| r.exam_attempt_id)
        .collect();
    // For all expired attempts, create a moderation entry
    // 1. Get all exams
    // 2. Find all attempts where `(attempt.startTimeInMS + exam.config.totalTimeInMS) < now`
    let mut exams = exam_collection.find(doc! {}).await?;
    while let Some(exam) = exams.next().await {
        let exam = exam?;
        let total_time_in_ms = exam.config.total_time_in_m_s as i64;
        tracing::debug!("Checking exam: {}", exam.id);

        // Get all attempts for this exam where the attempt id is not in the moderation collection
        let mut attempts = attempt_collection
            .find(doc! {
              "examId": exam.id,
              "_id": {
                  "$nin": &exam_attempt_ids
              }
            })
            .projection(doc! {"_id": true, "startTimeInMS": true})
            .await?;

        while let Some(attempt) = attempts.next().await {
            let attempt = attempt?;
            let start_time_in_ms = attempt.start_time_in_m_s as i64;
            let expiry_time_in_ms = start_time_in_ms + total_time_in_ms;
            let now = chrono::Utc::now();
            let expired = expiry_time_in_ms < now.timestamp_millis();

            tracing::debug!(
                "Attempt {} expires at: {:?}",
                attempt.id,
                chrono::Utc.timestamp_millis_opt(expiry_time_in_ms)
            );
            if expired {
                tracing::info!("Creating moderation entry for attempt: {}", attempt.id);
                let exam_moderation = ExamEnvironmentExamModeration {
                    id: ObjectId::new(),
                    exam_attempt_id: attempt.id,
                    moderator_id: None,
                    status: ExamEnvironmentExamModerationStatus::Pending,
                    feedback: None,
                    moderation_date: None,
                    submission_date: now,
                };

                // Create a moderation entry
                moderation_collection.insert_one(exam_moderation).await?;
            }
        }
    }
    Ok(())
}
