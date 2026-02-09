use anyhow::Context;
use exam_utils::misc::check_attempt_pass;
use futures_util::TryStreamExt;
use mongodb::{
    Client, Namespace,
    bson::{self, doc, oid::ObjectId},
};
use prisma::{db::get_collection, *};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

/// Finds all approved moderations between set dates `gt` and `lt`
/// Gets matching attempt
/// Checks attempt passes
/// Gets user
/// Pushes completed challenge, if not already present
pub async fn award_challenges_from_date(client: Client) -> anyhow::Result<()> {
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
    // gt 13:00 05/02/2026 && lt 06:00 06/02/2026
    let gt = bson::DateTime::builder()
        .year(2026)
        .month(2)
        .day(5)
        .hour(13)
        .build()?;
    let lt = bson::DateTime::builder()
        .year(2026)
        .month(2)
        .day(6)
        .hour(7)
        .build()?;
    info!(?gt, ?lt);
    let attempt_ids: Vec<AttemptId> = moderation_collection
        .clone_with_type::<AttemptId>()
        .find(doc! {
            "challengesAwarded": true,
            "status": ExamEnvironmentExamModerationStatus::Approved,
            "$and": [{ "moderationDate": { "$gt": gt } }, { "moderationDate": { "$lt": lt } }]
        })
        .projection(doc! {
            "examAttemptId": true
        })
        .await?
        .try_collect()
        .await?;

    info!(num = attempt_ids.len());

    let attempts = attempt_collection
        .find(doc! {
            "_id": {
                "$in": attempt_ids.iter().map(|id| id.exam_attempt_id).collect::<Vec<_>>()
            }
        })
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
            .find(doc! { "examId": { "$in": &unique_exam_ids } })
            .await?
            .try_collect()
            .await?;

    // Construct CompletedChallenge update for `user_id` pushing `challenge_id` if `exam_id` matches, and `challenge_id` is not already in `user.completedChallenges[].id`
    let exams = exam_collection
        .find(doc! { "_id": { "$in": unique_exam_ids } })
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let generated_exams = generated_exam_collection
        .find(doc! { "_id": { "$in": unique_generated_exam_ids } })
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

        info!(
            attempt = %attempt.id,
            user = %attempt.user_id,
            exam = %attempt.exam_id,
            pass
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
                warn!(
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
                .filter(doc! { "_id": attempt.user_id, "completedChallenges.id": { "$ne": &id } })
                .update(doc! { "$push": { "completedChallenges": &completed_bson } })
                .build(),
        );
    }

    if !updates.is_empty() {
        let res = user_collection.client().bulk_write(updates).await?;

        info!(
            "Updated {} users with new challenge IDs",
            res.modified_count
        );
    }

    Ok(())
}
