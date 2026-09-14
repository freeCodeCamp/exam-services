use std::collections::HashMap;

use anyhow::Context;
use futures_util::{StreamExt, TryStreamExt};
use mongodb::{
    Namespace,
    bson::{DateTime, doc, oid::ObjectId},
};
use serde::{Deserialize, Serialize};

use exam_utils::{attempt::construct_attempt, misc::check_attempt_pass, moderation_versions};
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

    let mut num_attempts_scanned = 0;
    let mut num_attempts_practice_skipped = 0;
    let mut num_attempts_expired = 0;
    let mut num_attempts_passed = 0;
    let mut num_attempts_failed_auto_approved = 0;
    let mut num_attempts_below_moderation_threshold = 0;
    let mut num_attempts_above_moderation_threshold = 0;
    let mut num_score_errors = 0;
    let mut num_attempts_event_fetch_failed = 0;

    while let Some(attempt) = attempts_cursor.next().await {
        let attempt = attempt.context("unable to deserialize attempt to collection")?;
        num_attempts_scanned += 1;

        if attempt.exam_id == practice_exam_id {
            num_attempts_practice_skipped += 1;
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

        let submission_date =
            DateTime::from_millis(attempt.start_time.timestamp_millis() + total_time_in_ms);

        if expired {
            num_attempts_expired += 1;
            let mut exam_moderation = ExamEnvironmentExamModeration {
                id: ObjectId::new(),
                exam_attempt_id: attempt.id,
                moderator_id: None,
                status: ExamEnvironmentExamModerationStatus::Pending,
                feedback: None,
                moderation_date: None,
                submission_date,
                challenges_awarded: false,
                moderation_score: None,
                // TODO: This should not be set outside of prisma in `freeCodeCamp/freeCodeCamp/api`
                version: 3,
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
                num_attempts_failed_auto_approved += 1;
                exam_moderation.status = ExamEnvironmentExamModerationStatus::Approved;
                exam_moderation.moderation_date = Some(now);
                exam_moderation.feedback = Some("Auto Approved - Failed attempt".to_string());
                // Set to true to avoid another check for whether the attempt passed or not.
                exam_moderation.challenges_awarded = true;
            } else {
                num_attempts_passed += 1;
                let events = match get_events_for_attempt(&supabase, &attempt.id).await {
                    Ok(events) => events,
                    Err(e) => {
                        // Skip this attempt without creating a moderation record. The attempt
                        // keeps a null `examModerationId`, so the next cron run picks it up again.
                        num_attempts_event_fetch_failed += 1;
                        tracing::error!(
                            attempt_id = %attempt.id,
                            error = ?e,
                            "unable to get events for attempt - skipping attempt until next run"
                        );
                        continue;
                    }
                };

                let attempt = construct_attempt(&exam, &generated_exam, &attempt);

                // Score under every registered algorithm version for comparison in Sentry. Only LIVE version score is persisted and drives placement.
                let mut scores = HashMap::new();
                for version in moderation_versions::VERSIONS {
                    let start = std::time::Instant::now();
                    let result = (version.score)(&attempt, &events);
                    sentry::metrics::distribution(
                        "exam_service.moderation_score_duration",
                        start.elapsed().as_secs_f64() * 1000.0,
                    )
                    .unit(sentry::protocol::Unit::Millisecond)
                    .attribute("version", version.label)
                    .capture();

                    match &result {
                        Ok(moderation_score) => {
                            sentry::metrics::distribution(
                                "exam_service.moderation_score",
                                *moderation_score,
                            )
                            .attribute("version", version.label)
                            .capture();
                        }
                        Err(e) => {
                            tracing::error!(attempt = %attempt.id, version = version.label, error = %e, "unable to calculate moderation score");
                        }
                    }

                    scores.insert(version.id, result);
                }

                let live_result = scores
                    .remove(&moderation_versions::LIVE)
                    .context("live moderation-score version must be registered")?;

                match live_result {
                    Ok(moderation_score) => {
                        exam_moderation.moderation_score = Some(moderation_score);
                        if moderation_score < env_vars.moderation_threshold {
                            num_attempts_below_moderation_threshold += 1;
                            exam_moderation.status = ExamEnvironmentExamModerationStatus::Approved;
                            exam_moderation.moderation_date = Some(now);
                            exam_moderation.feedback = Some(format!("Auto Approved"));
                        } else {
                            num_attempts_above_moderation_threshold += 1;
                        }
                    }
                    Err(_) => {
                        num_score_errors += 1;
                        exam_moderation.feedback =
                            Some(format!("Moderation score calculation error."));
                    }
                };
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

    sentry::metrics::counter("exam_service.attempts_scanned", num_attempts_scanned).capture();
    sentry::metrics::counter(
        "exam_service.attempts_practice_skipped",
        num_attempts_practice_skipped,
    )
    .capture();
    sentry::metrics::counter("exam_service.attempts_expired", num_attempts_expired).capture();
    sentry::metrics::counter("exam_service.attempts_passed", num_attempts_passed).capture();
    sentry::metrics::counter(
        "exam_service.attempts_failed_auto_approved",
        num_attempts_failed_auto_approved,
    )
    .capture();
    sentry::metrics::counter(
        "exam_service.below_threshold",
        num_attempts_below_moderation_threshold,
    )
    .capture();
    sentry::metrics::counter(
        "exam_service.above_threshold",
        num_attempts_above_moderation_threshold,
    )
    .capture();
    sentry::metrics::counter("exam_service.score_errors", num_score_errors).capture();
    sentry::metrics::counter(
        "exam_service.attempts_event_fetch_failed",
        num_attempts_event_fetch_failed,
    )
    .capture();

    Ok(())
}

/// Records the outcome of a single Supabase request as metrics, so failures can be
/// attributed to the exact table and operation - not just the error message.
fn record_supabase_request(
    table: &'static str,
    operation: &'static str,
    elapsed_ms: f64,
    ok: bool,
) {
    sentry::metrics::distribution("exam_service.supabase_request_duration", elapsed_ms)
        .unit(sentry::protocol::Unit::Millisecond)
        .attribute("table", table)
        .attribute("operation", operation)
        .capture();
    if !ok {
        sentry::metrics::counter("exam_service.supabase_request_failed", 1)
            .attribute("table", table)
            .attribute("operation", operation)
            .capture();
    }
}

#[tracing::instrument(
    skip_all,
    fields(
        supabase.table = "events",
        supabase.operation = "select",
        supabase.filter = %format!("attempt_id=eq.{}", attempt_id.to_hex()),
    ),
    err(Debug)
)]
async fn get_events_for_attempt(
    supabase: &SupabaseClient,
    attempt_id: &ObjectId,
) -> anyhow::Result<Vec<Event>> {
    let attempt_id = attempt_id.to_hex();
    let query = format!("select events where attempt_id=eq.{attempt_id}");

    let start = std::time::Instant::now();
    let result = supabase
        .from("events")
        .eq("attempt_id", &attempt_id)
        .execute()
        .await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    record_supabase_request("events", "select", elapsed_ms, result.is_ok());

    let rows = match result {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(
                supabase.query = %query,
                attempt_id = %attempt_id,
                duration_ms = elapsed_ms,
                error = %e,
                "supabase query failed"
            );
            return Err(anyhow::Error::msg(e).context(format!("supabase query failed: {query}")));
        }
    };

    let num_rows = rows.len();
    let mut num_deserialize_errors = 0;
    let events: Vec<Event> = rows
        .into_iter()
        .filter_map(|event| match serde_json::from_value(event) {
            Ok(event) => Some(event),
            Err(e) => {
                num_deserialize_errors += 1;
                tracing::warn!(
                    supabase.query = %query,
                    attempt_id = %attempt_id,
                    error = ?e,
                    "unable to deserialize event"
                );
                None
            }
        })
        .collect();

    if num_deserialize_errors > 0 {
        sentry::metrics::counter(
            "exam_service.supabase_row_deserialize_errors",
            num_deserialize_errors as f64,
        )
        .attribute("table", "events")
        .capture();
    }

    tracing::debug!(
        supabase.query = %query,
        attempt_id = %attempt_id,
        duration_ms = elapsed_ms,
        num_rows,
        num_events = events.len(),
        "supabase query succeeded"
    );

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

    // Current pending backlog awaiting moderation.
    sentry::metrics::gauge(
        "exam_service.pending_backlog",
        moderation_records.len() as f64,
    )
    .capture();

    let now = DateTime::now();

    let mut num_auto_approved_expired = 0;

    // If moderation record is pending, and is older than set moderation length, approve
    for moderation in moderation_records.iter() {
        let submission_date = moderation.submission_date;
        let expiry_date = submission_date.saturating_add_duration(env_vars.moderation_length_in_s);
        if now > expiry_date {
            num_auto_approved_expired += 1;
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

    sentry::metrics::counter(
        "exam_service.auto_approved_expired",
        num_auto_approved_expired,
    )
    .capture();

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

    let mut num_challenge_not_found = 0;
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
                num_challenge_not_found += 1;
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

        sentry::metrics::counter("exam_service.users_awarded", res.modified_count as f64).capture();
    }

    // Finally, update all moderation records to set challengesAwarded to true where status is approved and challengesAwarded is false
    let update_result = moderation_collection
        .update_many(
            doc! {"challengesAwarded": false, "status": ExamEnvironmentExamModerationStatus::Approved},
            doc! {"$set": {"challengesAwarded": true}},
        )
        .await
        .context("unable to update moderation records to set challengesAwarded to true")?;
    sentry::metrics::counter(
        "exam_service.challenges_awarded",
        update_result.modified_count as f64,
    )
    .capture();
    sentry::metrics::counter("exam_service.challenge_not_found", num_challenge_not_found).capture();

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

    sentry::metrics::counter(
        "exam_service.practice_attempts_deleted",
        delete_result.deleted_count as f64,
    )
    .capture();

    Ok(())
}

/// Delete Supabase events older than 30 days
#[tracing::instrument(
    skip_all,
    fields(
        supabase.table = "events",
        supabase.operation = "delete",
        supabase.filter = tracing::field::Empty,
    ),
    err(Debug)
)]
pub async fn delete_supabase_events(env_vars: &EnvVars) -> anyhow::Result<()> {
    let supabase_url = &env_vars.supabase_url;
    let supabase_key = &env_vars.supabase_key;
    // let supabase = SupabaseClient::new(supabase_url, supabase_key)?;
    let client = postgrest::Postgrest::new(format!("{supabase_url}/rest/v1"))
        .insert_header("apikey", supabase_key)
        .insert_header("Prefer", "return=representation");

    let expiry_date = chrono::Utc::now() - chrono::Duration::days(30);
    let query = format!(
        "delete events where timestamp=lt.{}",
        expiry_date.to_rfc3339()
    );
    tracing::Span::current().record("supabase.filter", tracing::field::display(&query));

    let start = std::time::Instant::now();
    let result = client
        .from("events")
        .lt("timestamp", &expiry_date.to_rfc3339())
        .delete()
        .execute()
        .await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let res = match result.and_then(|res| res.error_for_status()) {
        Ok(res) => {
            record_supabase_request("events", "delete", elapsed_ms, true);
            res
        }
        Err(e) => {
            record_supabase_request("events", "delete", elapsed_ms, false);
            let status = e.status().map(|s| s.as_u16());
            tracing::error!(
                supabase.query = %query,
                duration_ms = elapsed_ms,
                status = ?status,
                error = %e,
                "supabase query failed"
            );
            return Err(anyhow::Error::new(e).context(format!("supabase query failed: {query}")));
        }
    };

    let text = res
        .text()
        .await
        .context(format!("unable to read response body for: {query}"))?;
    let json: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&text);
    match json {
        Ok(v) => {
            sentry::metrics::counter("exam_service.supabase_events_deleted", v.len() as f64)
                .capture();
            tracing::debug!(
                supabase.query = %query,
                duration_ms = elapsed_ms,
                num_rows = v.len(),
                "supabase query succeeded"
            );
        }
        Err(e) => {
            sentry::metrics::counter("exam_service.supabase_events_parse_errors", 1).capture();
            tracing::warn!(supabase.query = %query, error = %e, text, "unable to serialize response as json array");
        }
    };

    Ok(())
}
