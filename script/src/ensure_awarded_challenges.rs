use std::{collections::HashMap, str::FromStr};

use exam_utils::misc::check_attempt_pass;
use futures_util::{StreamExt, TryStreamExt};
use indicatif::ProgressBar;
use mongodb::{
    Client,
    bson::{Bson, Document, deserialize_from_document, doc, oid::ObjectId, serialize_to_bson},
};
use serde_json::{Value, json};
use tracing::{debug, error, info, trace, warn};

use prisma::db::{get_collection, get_from_cache_or_collection};

/// Finds all approved moderations with `challengesAwarded: true`
/// Gets matching attempt
/// Checks attempt passes
/// Gets user
/// Pushes completed challenge, if not already present
pub async fn ensure_awarded_challenges(client: Client) -> Result<(), String> {
    let moderation_col = get_collection::<prisma::ExamEnvironmentExamModeration>(
        &client,
        "ExamEnvironmentExamModeration",
    )
    .await;
    let exam_col =
        get_collection::<prisma::ExamEnvironmentExam>(&client, "ExamEnvironmentExam").await;
    let generation_col = get_collection::<prisma::ExamEnvironmentGeneratedExam>(
        &client,
        "ExamEnvironmentGeneratedExam",
    )
    .await;
    let challenge_col =
        get_collection::<prisma::ExamEnvironmentChallenge>(&client, "ExamEnvironmentChallenge")
            .await;
    let user_col = get_collection::<Document>(&client, "user").await;

    let moderation_filter = doc! {"status": prisma::ExamEnvironmentExamModerationStatus::Approved, "challengesAwarded": true};

    let moderation_count = moderation_col
        .count_documents(moderation_filter.clone())
        .await
        .map_err(err("unable to query moderation count"))?;

    let pb: ProgressBar = ProgressBar::new(moderation_count);
    let mut num_attempts_need_fixing = 0;
    let mut num_attempts_not_need_fixing = 0;

    let mut exams: HashMap<ObjectId, prisma::ExamEnvironmentExam> = HashMap::new();
    let mut generations = HashMap::<ObjectId, prisma::ExamEnvironmentGeneratedExam>::new();
    let mut challenges = HashMap::<ObjectId, Vec<ObjectId>>::new();

    let pipeline = vec![
        doc! { "$match": moderation_filter },
        doc! { "$lookup": {
            "from": "ExamEnvironmentExamAttempt",
            "localField": "examAttemptId",
            "foreignField": "_id",
            "as": "attempt"
        }},
        doc! { "$unwind": "$attempt" },
    ];

    let mut cursor = moderation_col
        .aggregate(pipeline)
        .await
        .map_err(err("unable to run lookup aggregation"))?;

    while let Some(mod_attempt) = cursor.next().await {
        pb.inc(1);
        let attempt = match mod_attempt {
            Ok(mod_attempt) => {
                let attempt: prisma::ExamEnvironmentExamAttempt = match mod_attempt
                    .get_document("attempt")
                {
                    Ok(a) => match deserialize_from_document(a.clone()) {
                        Ok(a) => a,
                        Err(e) => {
                            error!(error = ?e, "unable to deserialize attempt");
                            continue;
                        }
                    },
                    Err(e) => {
                        error!(
                            error = ?e, "'attempt' field not attached as document to moderation attempt"
                        );
                        continue;
                    }
                };
                attempt
            }
            Err(e) => {
                error!(error = ?e, "unable to access moderation attempt");
                continue;
            }
        };

        // Check attempt passed
        let exam = match get_from_cache_or_collection(
            &exam_col,
            doc! {"_id": attempt.exam_id},
            &mut exams,
            attempt.exam_id,
        )
        .await
        {
            Some(e) => e,
            None => continue,
        };

        let generation = match get_from_cache_or_collection(
            &generation_col,
            doc! {"_id": attempt.generated_exam_id},
            &mut generations,
            attempt.generated_exam_id,
        )
        .await
        {
            Some(g) => g,
            None => continue,
        };

        let passed = check_attempt_pass(&exam, &generation, &attempt);

        if !passed {
            debug!(attempt_id = %attempt.id, "attempt did not pass");
            continue;
        }
        debug!(attempt_id = %attempt.id, "attempt passed");

        // Get user
        let user = match user_col
            .find_one(doc! {"_id": attempt.user_id})
            .projection(doc! {"_id": true,"completedChallenges": true})
            .await
        {
            Ok(user) => match user {
                Some(user) => user,
                None => {
                    error!(
                       user_id = %attempt.user_id, attempt_id = %attempt.id, "user does not exist for attempt",
                    );
                    continue;
                }
            },
            Err(e) => {
                error!(
                    error = ?e, user_id = %attempt.user_id, attempt_id = %attempt.id, "unable to find user for attempt"
                );
                continue;
            }
        };

        let challenges = if let Some(challenges) = challenges.get(&exam.id) {
            challenges.to_owned()
        } else {
            let chals: mongodb::Cursor<prisma::ExamEnvironmentChallenge> =
                if let Ok(c) = challenge_col.find(doc! {"examId": exam.id}).await {
                    c
                } else {
                    error!(exam_id = %exam.id, "unable to query challenges for exam id");
                    continue;
                };
            let cs: Vec<ObjectId> = if let Ok(ids) = chals
                .map(|c| match c {
                    Ok(c) => Ok(c.challenge_id),
                    Err(e) => Err(e),
                })
                .try_collect()
                .await
            {
                ids
            } else {
                error!(
                    exam_id = %exam.id, "unable to deserialize challenges from query for exam id"
                );
                continue;
            };
            challenges.insert(exam.id, cs.clone());
            cs
        };

        // Check if challenges are all awarded
        let mut challenges_to_award = vec![];
        let mut awarded_challenges = vec![];
        match user.get_array("completedChallenges") {
            Ok(completed_challenges) => {
                for completed_challenge in completed_challenges {
                    if let Some(completed_challenge) = completed_challenge.as_document() {
                        let id = match completed_challenge.get("id") {
                            Some(Bson::ObjectId(id)) => *id,
                            Some(Bson::String(s)) => {
                                if let Ok(o) = ObjectId::from_str(s) {
                                    o
                                } else {
                                    warn!(id = s, "unable to parse completed challenge id");
                                    continue;
                                }
                            }
                            t => {
                                warn!(
                                    field = ?t, user_id = %attempt.user_id, "unexpected completed challenge id type for user"
                                );
                                continue;
                            }
                        };

                        awarded_challenges.push(id);
                    } else {
                        warn!(user_id = %attempt.user_id, "completed challenge is not document");
                    }
                }
            }
            Err(e) => {
                error!(user_id = %attempt.user_id, error = ?e, "unexpected type for completed challenges field");
            }
        }

        for challenge in challenges {
            if !awarded_challenges.contains(&challenge) {
                challenges_to_award.push(challenge);
                trace!(%challenge, user_id = %attempt.user_id, "challenge needing awarding to user");
            } else {
                trace!(%challenge, user_id = %attempt.user_id, "challenge already awarded to user");
            }
        }

        if !challenges_to_award.is_empty() {
            num_attempts_need_fixing += 1;

            let attempt_start_time = attempt.start_time.timestamp_millis();

            let chals: Vec<Value> = challenges_to_award
                .iter()
                .map(|id| {
                    json!({
                        "id": id.to_hex(),
                        "completedDate": attempt_start_time,
                        "challengeType": json!(30)
                    })
                })
                .collect();

            info!(user_id = %attempt.user_id, num_challenges = challenges_to_award.len(), "awarding challenges");
            let completed_bson = match serialize_to_bson(&json!(chals)) {
                Ok(b) => b,
                Err(e) => {
                    error!(error = ?e, "unable to serialize challenges to bson");
                    continue;
                }
            };
            info!(user_id = %attempt.user_id, "updating user with completed challenges");
            if let Err(e) = user_col
                .update_one(
                    doc! {"_id": attempt.user_id},
                    doc! {"$push": {"completedChallenges": {"$each": completed_bson}}},
                )
                .await
            {
                error!(
                    user_id = %attempt.user_id, ?challenges_to_award, error = ?e,"unable to update user with challenges",
                );
                continue;
            }
        } else {
            num_attempts_not_need_fixing += 1;
        }
    }

    pb.finish_with_message("challenges awarded");
    println!("Number of attempts fixed: {num_attempts_need_fixing}");
    println!("Number of attempts already handled: {num_attempts_not_need_fixing}");

    Ok(())
}

fn err<E>(s: &str) -> impl FnOnce(E) -> String
where
    E: ToString,
{
    return move |e: E| format!("{}: {}", s, e.to_string());
}
