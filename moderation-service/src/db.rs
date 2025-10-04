use anyhow::Context;
use futures_util::{StreamExt, TryStreamExt};
use mongodb::{
    Client, Collection, Namespace,
    bson::{DateTime, doc, oid::ObjectId},
    options::ClientOptions,
};
use serde::{Deserialize, Serialize};

use prisma::{
    CompletedChallenge, ExamEnvironmentChallenge, ExamEnvironmentConfig, ExamEnvironmentExam,
    ExamEnvironmentExamAttempt, ExamEnvironmentExamModeration, ExamEnvironmentExamModerationStatus,
    ExamEnvironmentGeneratedExam,
};

use crate::{attempt::check_attempt_pass, config::EnvVars};

const PRACTICE_EXAM_ID: &str = "674819431ed2e8ac8d170f5e";

pub async fn get_collection<'d, T>(client: &Client, collection_name: &str) -> Collection<T>
where
    T: Send + Sync + Deserialize<'d> + Serialize,
{
    let db = client
        .default_database()
        .expect("database needs to be defined in the URI");

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
        .default_database()
        .expect("database needs to be defined in the URI")
        // .database("freecodecamp")
        .run_command(doc! {"ping": 1})
        .await?;

    Ok(client)
}

/// Auto approves old, unmoderated moderation records
/// Creates moderation records for attempts not already in the queue
/// Finds approved moderation records and awards the user their certificate
#[tracing::instrument(skip_all, err(Debug))]
pub async fn update_moderation_collection(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let moderation_collection =
        get_collection::<ExamEnvironmentExamModeration>(&client, "ExamEnvironmentExamModeration")
            .await;
    let attempt_collection =
        get_collection::<ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt").await;
    let exam_collection =
        get_collection::<ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;

    let now = DateTime::now();

    #[derive(Deserialize)]
    struct ExamEnvironmentExamModerationProjection {
        // #[serde(rename = "_id")]
        // id: ObjectId,
        #[serde(rename = "examAttemptId")]
        exam_attempt_id: ObjectId,
    }

    let moderation_records: Vec<ExamEnvironmentExamModerationProjection> = moderation_collection
        .clone_with_type::<ExamEnvironmentExamModerationProjection>()
        .find(doc! {})
        .projection(doc! {"examAttemptId": true, "_id": true})
        .await
        .context("unable to find moderation records")?
        .try_collect()
        .await
        .context("unable to deserialize moderation records to projection")?;

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

        let practice_exam_id =
            ObjectId::parse_str(PRACTICE_EXAM_ID).expect("static str is valid object id");
        if exam.id == practice_exam_id {
            tracing::debug!("Skipping practice exam: {}", exam.id);
            continue;
        }

        let total_time_in_ms = exam
            .config
            .total_time_in_s
            .unwrap_or(exam.config.total_time_in_m_s * 1000);
        tracing::debug!("Checking exam: {}", exam.id);

        #[derive(Deserialize)]
        struct ExamEnvironmentExamAttemptProjection {
            #[serde(rename = "_id")]
            id: ObjectId,
            #[serde(rename = "startTimeInMS")]
            start_time_in_m_s: i64,
            #[serde(rename = "startTime")]
            start_time: Option<DateTime>,
        }
        // Get all attempts for this exam where the attempt id is not in the moderation collection
        // TODO: Also, where the attempt was passed.
        let mut attempts = attempt_collection
            .clone_with_type::<ExamEnvironmentExamAttemptProjection>()
            // _id must not be in existing exam attempts
            .find(doc! {
              "examId": exam.id,
              "_id": {
                  "$nin": &exam_attempt_ids
              }
            })
            .projection(doc! {"_id": true, "startTimeInMS": true, "startTime": true})
            .await
            .context("unable to find attempts")?;

        while let Some(attempt) = attempts.next().await {
            let attempt = attempt.context("unable to deserialize attempt to collection")?;
            let start_time_in_ms = attempt
                .start_time
                .map_or(attempt.start_time_in_m_s, |dt| dt.timestamp_millis());
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

/// Auto approves old, unmoderated moderation records
#[tracing::instrument(skip_all, err(Debug))]
pub async fn auto_approve_moderation_records(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let moderation_collection =
        get_collection::<ExamEnvironmentExamModeration>(&client, "ExamEnvironmentExamModeration")
            .await;

    #[derive(Deserialize)]
    struct ExamEnvironmentExamModerationProjection {
        #[serde(rename = "_id")]
        id: ObjectId,
        #[serde(rename = "submissionDate")]
        submission_date: DateTime,
    }
    // Find pending moderation records
    let moderation_records: Vec<ExamEnvironmentExamModerationProjection> = moderation_collection
        .clone_with_type::<ExamEnvironmentExamModerationProjection>()
        .find(doc! {
            "status": ExamEnvironmentExamModerationStatus::Pending
        })
        .projection(doc! { "_id": true, "submissionDate": true})
        .await
        .context("unable to find moderation records")?
        .try_collect()
        .await
        .context("unable to deserialize moderation records to projection")?;

    let now = DateTime::now();

    // If moderation record is pending, and is older than set moderation length, approve
    for moderation in moderation_records.iter() {
        let submission_date = moderation.submission_date;
        let expiry_date = submission_date.saturating_add_duration(env_vars.moderation_length_in_s);
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

    Ok(())
}

#[derive(Deserialize, Serialize)]
struct User {
    #[serde(rename = "_id")]
    id: ObjectId,
    #[serde(rename = "completedChallenges")]
    completed_challenges: Vec<prisma::CompletedChallenge>,
}

/// Awards certification (challenge) IDs to users:
/// 1. Finds all approved moderation records where challengesAwarded is false
/// 2. Finds the associated exam attempt, and from that the user ID and exam ID
/// 3. Finds the challenge ID associated with the exam ID
/// 4. Updates the user record to add the challenge ID to completedChallenges if not already present
/// 5. Sets challengesAwarded to true on the moderation record
#[tracing::instrument(skip_all, err(Debug))]
pub async fn award_challenge_ids(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let moderation_collection =
        get_collection::<ExamEnvironmentExamModeration>(&client, "ExamEnvironmentExamModeration")
            .await;
    let attempt_collection =
        get_collection::<ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt").await;
    let exam_collection =
        get_collection::<ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;
    let generated_exam_collection =
        get_collection::<ExamEnvironmentGeneratedExam>(&client, "ExamEnvironmentGeneratedExam")
            .await;
    let exam_environment_challenge_collection =
        get_collection::<ExamEnvironmentChallenge>(&client, "ExamEnvironmentChallenge").await;
    let user_collection = get_collection::<User>(&client, "user").await;

    #[derive(Deserialize)]
    struct AttemptId {
        #[serde(rename = "examAttemptId")]
        pub exam_attempt_id: ObjectId,
    }
    let attempt_ids: Vec<AttemptId> = moderation_collection.clone_with_type::<AttemptId>().find(doc!{"challengesAwarded": false, "status": ExamEnvironmentExamModerationStatus::Approved}).projection(doc!{"examAttemptId": true, "startTime": true}).await?.try_collect().await?;

    let attempts = attempt_collection.find(doc!{"_id": {"$in": attempt_ids.iter().map(|id| id.exam_attempt_id).collect::<Vec<_>>()}})
        .await?
        .try_collect::<Vec<_>>()
        .await?;

    let unique_exam_ids = attempts
        .iter()
        .map(|a| a.exam_id)
        .collect::<std::collections::HashSet<_>>();
    let unique_generated_exam_ids = attempts
        .iter()
        .map(|a| a.generated_exam_id)
        .collect::<std::collections::HashSet<_>>();

    tracing::debug!("Unique exam IDs: {:?}", unique_exam_ids);

    let exam_environment_challenges: Vec<ExamEnvironmentChallenge> =
        exam_environment_challenge_collection
            .find(doc! {"examId": {"$in": &unique_exam_ids}})
            .await?
            .try_collect()
            .await?;

    // Construct CompletedChallenge update for `user_id` pushing `challenge_id` if `exam_id` matches, and `challenge_id` is not already in `user.completedChallenges[].id`
    let exams = exam_collection
        .find(doc! {"_id": {"$in": unique_exam_ids}})
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let generated_exams = generated_exam_collection
        .find(doc! {"_id": {"$in": unique_generated_exam_ids}})
        .await?
        .try_collect::<Vec<_>>()
        .await?;

    let mut updates = vec![];
    for attempt in attempts {
        // Check attempt passes exam:
        let exam = exams
            .iter()
            .find(|e| e.id == attempt.exam_id)
            .expect("exam must exist for attempt");
        let generated_exam = generated_exams
            .iter()
            .find(|ge| ge.id == attempt.generated_exam_id)
            .expect("generated exam must exist for attempt");
        let pass = check_attempt_pass(&exam, &generated_exam, &attempt);

        tracing::debug!(
            attempt_id = attempt.id.to_hex(),
            exam_id = attempt.exam_id.to_hex(),
            "Attempt passed: {pass}"
        );
        if !pass {
            continue;
        }

        let completed_date = attempt
            .start_time
            .expect("migration to have been run")
            .timestamp_millis()
            .into();
        let id = match exam_environment_challenges
            .iter()
            .find(|c| c.exam_id == attempt.exam_id)
        {
            Some(challenge) => challenge.challenge_id.to_hex(),
            None => {
                tracing::warn!(
                    user_id = attempt.user_id.to_hex(),
                    exam_id = attempt.exam_id.to_hex(),
                    "No challenge found to award user"
                );
                continue;
            }
        };
        let completed_challenge = CompletedChallenge {
            completed_date,
            id,
            challenge_type: Default::default(),
            files: Default::default(),
            github_link: Default::default(),
            is_manually_approved: Default::default(),
            solution: Default::default(),
            exam_results: Default::default(),
        };

        let namespace = Namespace::new("freecodecamp", "user");
        updates.push(mongodb::options::UpdateOneModel::builder().namespace(namespace)
            .filter(doc!{"_id": attempt.user_id, "completedChallenges.id": {"$ne": &completed_challenge.id}})
            .update(doc!{"$push": {"completedChallenges": mongodb::bson::serialize_to_bson(&completed_challenge)?}}).build());
    }

    if !updates.is_empty() {
        let res = user_collection.client().bulk_write(updates).await?;

        tracing::info!(
            "Updated {} users with new challenge IDs",
            res.modified_count
        );
    }

    // Finally, update all moderation records to set challengesAwarded to true where status is approved and challengesAwarded is false
    let update_result = moderation_collection
        .update_many(
            doc! {"challengesAwarded": false, "status": ExamEnvironmentExamModerationStatus::Approved},
            doc! {"$set": {"challengesAwarded": true}},
        )
        .await
        .context("unable to update moderation records to set challengesAwarded to true")?;
    tracing::info!(
        "Updated {} moderation records to set challengesAwarded to true",
        update_result.modified_count
    );

    Ok(())
}

#[tracing::instrument(skip_all, err(Debug))]
pub async fn delete_practice_exam_attempts(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let attempt_collection =
        get_collection::<ExamEnvironmentExamAttempt>(&client, "ExamEnvironmentExamAttempt").await;

    let practice_exam_id =
        ObjectId::parse_str(PRACTICE_EXAM_ID).expect("static str is valid object id");
    let delete_result = attempt_collection
        .delete_many(doc! {
            "examId": practice_exam_id
        })
        .await
        .context("unable to delete practice exam attempts")?;

    tracing::info!(
        "Deleted {} practice exam attempts",
        delete_result.deleted_count
    );

    Ok(())
}
