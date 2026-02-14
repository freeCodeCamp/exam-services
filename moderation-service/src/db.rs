use std::collections::HashMap;

use anyhow::Context;
use futures_util::{StreamExt, TryStreamExt};
use mongodb::{
    Namespace,
    bson::{DateTime, doc, oid::ObjectId},
};
use serde::{Deserialize, Serialize};

use exam_utils::{
    attempt::{construct_attempt, get_moderation_score},
    misc::check_attempt_pass,
};
use prisma::{
    ExamEnvironmentChallenge, ExamEnvironmentExam, ExamEnvironmentExamAttempt,
    ExamEnvironmentExamModeration, ExamEnvironmentExamModerationStatus,
    ExamEnvironmentGeneratedExam, db::*, supabase::Event,
};
use serde_json::json;
use supabase_rs::SupabaseClient;

use crate::config::EnvVars;

const PRACTICE_EXAM_ID: &str = "674819431ed2e8ac8d170f5e";

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
    let generation_collection =
        get_collection::<ExamEnvironmentGeneratedExam>(&client, "ExamEnvironmentGeneratedExam")
            .await;

    let supabase_url = &env_vars.supabase_url;
    let supabase_key = &env_vars.supabase_key;
    let supabase = SupabaseClient::new(supabase_url, supabase_key)?;

    let now = DateTime::now();

    let mut attempts_cursor = attempt_collection
        .find(doc! {
            "$or": [
                {
                    "examModerationId": {
                        "$exists": false
                    }
                },
                {
                    "examModerationId": null
                }
            ]
        })
        .await?;

    let mut exams = HashMap::new();
    let mut generated_exams = HashMap::new();
    let practice_exam_id =
        ObjectId::parse_str(PRACTICE_EXAM_ID).expect("static str is valid object id");

    while let Some(attempt) = attempts_cursor.next().await {
        let attempt = attempt.context("unable to deserialize attempt to collection")?;

        if attempt.exam_id == practice_exam_id {
            tracing::debug!(
                exam = %attempt.exam_id,
                "skipping practice exam"
            );
            continue;
        }

        let exam = if let Some(exam) = exams.get(&attempt.exam_id) {
            exam
        } else {
            let exam = exam_collection
                .find_one(doc! {"_id": &attempt.exam_id})
                .await?
                .context("unable to find exam for attempt")?;
            exams.insert(attempt.exam_id, exam);
            exams.get(&attempt.exam_id).unwrap()
        };

        let total_time_in_ms = exam.config.total_time_in_s * 1000;
        let start_time_in_ms = attempt.start_time.timestamp_millis();
        let expiry_time_in_ms = start_time_in_ms + total_time_in_ms;
        let expired = expiry_time_in_ms < now.timestamp_millis();

        tracing::debug!(
            attempt = %attempt.id,
            time = %DateTime::from_millis(expiry_time_in_ms),
            "attempt expiry",
        );

        let submission_date =
            DateTime::from_millis(attempt.start_time.timestamp_millis() + total_time_in_ms);

        if expired {
            tracing::debug!(
            attempt = %attempt.id,
                "creating moderation entry for attempt"
            );
            let mut exam_moderation = ExamEnvironmentExamModeration {
                id: ObjectId::new(),
                exam_attempt_id: attempt.id,
                moderator_id: None,
                status: ExamEnvironmentExamModerationStatus::Pending,
                feedback: None,
                moderation_date: None,
                submission_date,
                challenges_awarded: false,
                // TODO: This should not be set outside of prisma in `freeCodeCamp/freeCodeCamp/api`
                version: 2,
            };

            let generated_exam =
                if let Some(generated_exam) = generated_exams.get(&attempt.generated_exam_id) {
                    generated_exam
                } else {
                    let generated_exam = generation_collection
                        .find_one(doc! {"_id": &attempt.generated_exam_id})
                        .await?
                        .context("unable to find generated exam for attempt")?;
                    generated_exams.insert(attempt.generated_exam_id, generated_exam.clone());
                    &generated_exam.clone()
                };

            // If attempt failed, auto-moderate as approved with feedback
            let pass = check_attempt_pass(&exam, &generated_exam, &attempt);
            if !pass {
                tracing::debug!(
                    attempt = %attempt.id,
                    "attempt failed, setting moderation to approved",
                );
                exam_moderation.status = ExamEnvironmentExamModerationStatus::Approved;
                exam_moderation.moderation_date = Some(now);
                exam_moderation.feedback = Some("Auto Approved - Failed attempt".to_string());
                // Set to true to avoid another check for whether the attempt passed or not.
                exam_moderation.challenges_awarded = true;
            } else {
                let events = get_events_for_attempt(&supabase, &attempt.id).await?;

                let attempt = construct_attempt(&exam, &generated_exam, &attempt);
                let moderation_score = get_moderation_score(&attempt, &events)?;
                tracing::debug!(moderation_score, attempt = %attempt.id);

                if moderation_score < env_vars.moderation_threshold {
                    exam_moderation.status = ExamEnvironmentExamModerationStatus::Approved;
                    exam_moderation.moderation_date = Some(now);
                    exam_moderation.feedback = Some(format!(
                        "Auto Approved - Moderation score: {moderation_score}"
                    ));
                } else {
                    exam_moderation.feedback =
                        Some(format!("Moderation score: {moderation_score}"));
                }
            }

            // Create a moderation entry
            let res = moderation_collection
                .insert_one(&exam_moderation)
                .await
                .context("unable to insert moderation record")?;
            // Update the attempt to link to the moderation entry
            attempt_collection
                .update_one(
                    doc! {"_id": &attempt.id},
                    doc! {
                        "$set": {
                            "examModerationId": res.inserted_id
                        }
                    },
                )
                .await
                .context("unable to update attempt with moderation ID")?;
        }
    }
    Ok(())
}

#[tracing::instrument(skip_all, err(Debug))]
async fn get_events_for_attempt(
    supabase: &SupabaseClient,
    attempt_id: &ObjectId,
) -> anyhow::Result<Vec<Event>> {
    let events = supabase
        .from("events")
        .eq("attempt_id", &attempt_id.to_hex())
        .execute()
        .await
        .map_err(anyhow::Error::msg)
        .context("unable to get examts for attempt")?;

    let events: Vec<Event> = events
        .into_iter()
        .filter_map(|event| match serde_json::from_value(event) {
            Ok(event) => Some(event),
            Err(e) => {
                tracing::warn!(error = ?e, "unable to deserialize event");
                None
            }
        })
        .collect();
    Ok(events)
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
        tracing::debug!(moderation = %moderation.id, %expiry_date, "moderation expiry", );
        if now > expiry_date {
            tracing::info!(moderation = %moderation.id, "moderation auto-moderated");
            moderation_collection
                .update_one(
                    doc! {
                        "_id": moderation.id
                    },
                    doc! {
                        "$set": {
                            "feedback": "Auto Approved - Moderation time exceeded",
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

    tracing::debug!(?unique_exam_ids);

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
            .context("exam must exist for attempt")?;
        let generated_exam = generated_exams
            .iter()
            .find(|ge| ge.id == attempt.generated_exam_id)
            .context("generated exam must exist for attempt")?;
        let pass = check_attempt_pass(&exam, &generated_exam, &attempt);

        tracing::debug!(
            attempt_id = %attempt.id,
            exam_id = %attempt.exam_id,
            "Attempt passed: {pass}"
        );
        if !pass {
            continue;
        }

        let completed_date = attempt.start_time.timestamp_millis();
        let id = match exam_environment_challenges
            .iter()
            .find(|c| c.exam_id == attempt.exam_id)
        {
            Some(challenge) => challenge.challenge_id.to_hex(),
            None => {
                tracing::warn!(
                    user_id = %attempt.user_id,
                    exam_id = %attempt.exam_id,
                    "No challenge found to award user"
                );
                continue;
            }
        };
        let completed_challenge = json!({
            "id": &id,
            "completedDate": completed_date,
            // TODO: This is brittle
            "challengeType": Some(serde_json::json!(30)),
        });

        let completed_bson = mongodb::bson::serialize_to_bson(&completed_challenge)?;

        let namespace = Namespace::new("freecodecamp", "user");
        updates.push(
            mongodb::options::UpdateOneModel::builder()
                .namespace(namespace)
                .filter(doc! {"_id": attempt.user_id, "completedChallenges.id": {"$ne": &id}})
                .update(doc! {"$push": {"completedChallenges": &completed_bson}})
                .build(),
        );
    }

    if !updates.is_empty() {
        let res = user_collection.client().bulk_write(updates).await?;

        tracing::info!(
            num = res.modified_count,
            "updated users with new challenge IDs",
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
        num = update_result.modified_count,
        "updated moderation records to set challengesAwarded to true",
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
        "examId": practice_exam_id,
        "startTime": {
            // Long enough time for practice exam to expire
            "$lt": DateTime::from_system_time(std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(1000)).context(
                "unable to construct system time"
            )?)
        }
    })
    .await
    .context("unable to delete practice exam attempts")?;

    tracing::info!(
        num = delete_result.deleted_count,
        "deleted practice exam attempts",
    );

    Ok(())
}

/// Temporary task to sort out poorly created moderation record duplicates caused by upstream
/// - Partially fixed in upstream: https://github.com/freeCodeCamp/freeCodeCamp/pull/64635
/// - Fully fixed by: https://github.com/freeCodeCamp/freeCodeCamp/pull/64812
#[tracing::instrument(skip_all, err(Debug))]
pub async fn temp_handle_duplicate_moderations(env_vars: &EnvVars) -> anyhow::Result<()> {
    let client = client(&env_vars.mongodb_uri).await?;

    let moderation_collection =
        get_collection::<ExamEnvironmentExamModeration>(&client, "ExamEnvironmentExamModeration")
            .await;

    #[derive(Deserialize)]
    struct DuplicateAgg {
        _id: ObjectId,
    }

    // Aggregate on `examAttemptId` count
    let duplicate_records: Vec<DuplicateAgg> = moderation_collection
        .aggregate([
            doc! {
                "$group": {
                    "_id": "$examAttemptId",
                    "count": { "$sum": 1 }
                }
            },
            doc! {
                "$match": {
                    "count": {
                        "$gt": 1
                    }
                }
            },
        ])
        .with_type::<DuplicateAgg>()
        .await
        .context("unable to run duplicate moderation record aggregation")?
        .try_collect()
        .await?;

    for dup in duplicate_records {
        tracing::info!(?dup._id,"handling duplicate records");
        // Merge duplicates after getting them
        // 1. Change feedback to "Auto Moderated - Invalid attempt submission"
        // 2. Change status to "Denied"
        // 3. Use oldest submissionDate
        // 4. Set `challengesAwarded` to false
        // 5. Set moderatorId to `null`
        // 6. Set moderationDate to now
        let dups: Vec<ExamEnvironmentExamModeration> = moderation_collection
            .find(doc! {
                "examAttemptId": dup._id
            })
            .await?
            .try_collect()
            .await?;

        let mut updated_dup = ExamEnvironmentExamModeration {
            id: ObjectId::new(),
            status: ExamEnvironmentExamModerationStatus::Denied,
            exam_attempt_id: dup._id,
            feedback: Some("Auto Moderated - Invalid attempt submission".to_string()),
            moderation_date: Some(DateTime::now()),
            moderator_id: None,
            submission_date: DateTime::now(),
            challenges_awarded: true,
            version: 2,
        };

        let mut ids_to_delete = vec![];
        for dup in dups {
            if dup.submission_date < updated_dup.submission_date {
                updated_dup.submission_date = dup.submission_date;
            }
            ids_to_delete.push(dup.id);
        }

        let _res = moderation_collection.insert_one(updated_dup).await?;
        tracing::info!(attempt_id = %dup._id, "inserted updated moderation record");
        let del_res = moderation_collection
            .delete_many(doc! {
                "_id": {
                    "$in": &ids_to_delete
                }
            })
            .await?;

        assert_eq!(del_res.deleted_count, ids_to_delete.len() as u64);
        tracing::info!(attempt_id = %dup._id, "successfully deleted duplicate records");
    }

    Ok(())
}

/// Delete Supabase events older than 30 days
#[tracing::instrument(skip_all, err(Debug))]
pub async fn delete_supabase_events(env_vars: &EnvVars) -> anyhow::Result<()> {
    let supabase_url = &env_vars.supabase_url;
    let supabase_key = &env_vars.supabase_key;
    // let supabase = SupabaseClient::new(supabase_url, supabase_key)?;
    let client = postgrest::Postgrest::new(format!("{supabase_url}/rest/v1"))
        .insert_header("apikey", supabase_key)
        .insert_header("Prefer", "return=representation");

    let expiry_date = chrono::Utc::now() - chrono::Duration::days(30);
    tracing::info!(%expiry_date);

    let res = client
        .from("events")
        .lt("timestamp", &expiry_date.to_rfc3339())
        .delete()
        .execute()
        .await?
        .error_for_status()?;

    let text = res.text().await?;
    let json: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&text);
    match json {
        Ok(v) => {
            tracing::info!(num = v.len(), "deleted supabase rows");
        }
        Err(e) => {
            tracing::warn!(error = %e, text, "unable to serialize response as json array");
        }
    };

    Ok(())
}
