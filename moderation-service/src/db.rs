use anyhow::Context;
use bson::DateTime;
use futures_util::{StreamExt, TryStreamExt};
use mongodb::{
    Client, Collection,
    bson::{doc, oid::ObjectId},
    options::ClientOptions,
};
use serde::{Deserialize, Serialize};

use prisma::{
    ExamEnvironmentConfig, ExamEnvironmentExam, ExamEnvironmentExamAttempt,
    ExamEnvironmentExamModeration, ExamEnvironmentExamModerationStatus,
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

/// Auto approves old, unmoderated moderation records
/// Creates moderation records for attempts not already in the queue
#[tracing::instrument(skip_all)]
pub async fn update_moderation_collection(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let moderation_collection =
        get_collection::<ExamEnvironmentExamModeration>(&client, "ExamEnvironmentExamModeration")
            .await;
    let attempt_collection =
        get_collection::<ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt").await;
    let exam_collection =
        get_collection::<ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;

    #[derive(Deserialize)]
    struct ExamEnvironmentExamModerationProjection {
        #[serde(rename = "_id")]
        id: ObjectId,
        #[serde(rename = "examAttemptId")]
        exam_attempt_id: ObjectId,
        #[serde(rename = "submissionDate")]
        submission_date: DateTime,
        status: ExamEnvironmentExamModerationStatus,
    }
    let moderation_records: Vec<ExamEnvironmentExamModerationProjection> = moderation_collection
        .clone_with_type::<ExamEnvironmentExamModerationProjection>()
        .find(doc! {})
        .projection(
            doc! {"examAttemptId": true, "_id": true, "submissionDate": true, "status": true},
        )
        .await
        .context("unable to find moderation records")?
        .try_collect()
        .await
        .context("unable to deserialize moderation records to projection")?;

    let now = DateTime::now();

    // If moderation record is pending, and is older than set moderation length, approve
    for moderation in moderation_records.iter() {
        if let ExamEnvironmentExamModerationStatus::Pending = moderation.status {
            let submission_date = moderation.submission_date;
            let expiry_date =
                submission_date.saturating_add_duration(env_vars.moderation_length_in_s);
            tracing::debug!("Moderation {} expires at {}", moderation.id, expiry_date);
            if now > expiry_date {
                tracing::info!("Moderation {} auto-moderated", moderation.id);
                moderation_collection
                    .update_one(
                        doc! {
                            "_id": moderation.id
                        },
                        doc! {
                            "$set": {
                                "feedback": "Auto Approved",
                                "moderationDate": now,
                                "status": ExamEnvironmentExamModerationStatus::Approved
                            }
                        },
                    )
                    .await
                    .context("unable to auto-update moderation collection")?;
            }
        }
    }

    let exam_attempt_ids: Vec<ObjectId> = moderation_records
        .iter()
        .map(|r| r.exam_attempt_id)
        .collect();
    // For all expired attempts, create a moderation entry
    // 1. Get all exams
    // 2. Find all attempts where `(attempt.startTimeInMS + exam.config.totalTimeInMS) < now`
    #[derive(Deserialize)]
    struct ExamEnvironmentExamProjection {
        #[serde(rename = "_id")]
        id: ObjectId,
        config: ExamEnvironmentConfig,
    }
    let mut exams = exam_collection
        .clone_with_type::<ExamEnvironmentExamProjection>()
        .find(doc! {})
        .projection(doc! {"_id": true, "config": true})
        .await
        .context("unable to find exams")?;
    while let Some(exam) = exams.next().await {
        let exam = exam.context("unable to deserialize exam projection")?;
        let total_time_in_ms = exam.config.total_time_in_m_s as i64;
        tracing::debug!("Checking exam: {}", exam.id);

        #[derive(Deserialize)]
        struct ExamEnvironmentExamAttemptProjection {
            #[serde(rename = "_id")]
            id: ObjectId,
            #[serde(rename = "startTimeInMS")]
            start_time_in_m_s: usize,
        }
        // Get all attempts for this exam where the attempt id is not in the moderation collection
        let mut attempts = attempt_collection
            .clone_with_type::<ExamEnvironmentExamAttemptProjection>()
            .find(doc! {
              "examId": exam.id,
              "_id": {
                  "$nin": &exam_attempt_ids
              }
            })
            .projection(doc! {"_id": true, "startTimeInMS": true})
            .await
            .context("unable to find attempts")?;

        while let Some(attempt) = attempts.next().await {
            let attempt = attempt.context("unable to deserialize attempt to collection")?;
            let start_time_in_ms = attempt.start_time_in_m_s as i64;
            let expiry_time_in_ms = start_time_in_ms + total_time_in_ms;
            let expired = expiry_time_in_ms < now.timestamp_millis();

            tracing::debug!(
                "Attempt {} expires at: {:?}",
                attempt.id,
                DateTime::from_millis(expiry_time_in_ms)
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
                    // TODO: This should not be set outside of prisma in `freeCodeCamp/freeCodeCamp/api`
                    version: 1,
                };

                // Create a moderation entry
                moderation_collection
                    .insert_one(exam_moderation)
                    .await
                    .context("unable to insert moderation record")?;
            }
        }
    }
    Ok(())
}
