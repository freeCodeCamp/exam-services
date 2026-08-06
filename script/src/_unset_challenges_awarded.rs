use futures_util::TryStreamExt;
use mongodb::{
    Client,
    bson::{doc, oid::ObjectId},
};
use serde::Deserialize;
use tracing::{info, warn};

use prisma::db::get_collection;

/// Feedback written by the buggy auto-approve branch in `update_moderation_collection`.
///
/// Distinct from `"Auto Approved - Failed attempt"` (failed attempts, correctly flagged) and from
/// the pre-`bc6af64` `"Auto Approved - Moderation score: {score}"`, so it selects exactly the
/// passing, below-threshold attempts affected by the bug.
const BUGGED_FEEDBACK: &str = "Auto Approved";

/// Moderation record version introduced alongside the bug in `bc6af64`. Used as a second
/// discriminator so records from earlier releases cannot be selected.
const BUGGED_VERSION: i32 = 3;

/// Backfill for the `challengesAwarded` regression introduced in `bc6af64`.
///
/// `update_moderation_collection` set `challengesAwarded: true` at record creation for passing
/// attempts that auto-approved below the moderation threshold. `award_challenge_ids` builds its
/// work queue from `{challengesAwarded: false, status: Approved}`, so those records were never
/// processed and the users never received their `CompletedChallenge`.
///
/// - Finds approved moderations with `challengesAwarded: true` written by that branch
/// - Logs each affected record for audit
/// - Resets `challengesAwarded` to `false` so `award_challenge_ids` reprocesses them
///
/// Re-awarding is idempotent: `award_challenge_ids` filters on
/// `"completedChallenges.id": {"$ne": id}`, so users who already hold the challenge are untouched.
///
/// Runs as a dry run unless `CONFIRM=1` is set.
pub async fn _unset_challenges_awarded(client: Client) -> Result<(), String> {
    let moderation_col = get_collection::<prisma::ExamEnvironmentExamModeration>(
        &client,
        "ExamEnvironmentExamModeration",
    )
    .await;

    let filter = doc! {
        "status": prisma::ExamEnvironmentExamModerationStatus::Approved,
        "challengesAwarded": true,
        "feedback": BUGGED_FEEDBACK,
        "version": BUGGED_VERSION,
    };

    #[derive(Deserialize)]
    struct Affected {
        #[serde(rename = "_id")]
        id: ObjectId,
        #[serde(rename = "examAttemptId")]
        exam_attempt_id: ObjectId,
    }

    let affected: Vec<Affected> = moderation_col
        .clone_with_type::<Affected>()
        .find(filter.clone())
        .projection(doc! {"_id": true, "examAttemptId": true})
        .await
        .map_err(err("unable to query affected moderation records"))?
        .try_collect()
        .await
        .map_err(err("unable to deserialize affected moderation records"))?;

    if affected.is_empty() {
        println!("No affected moderation records found.");
        return Ok(());
    }

    for a in affected.iter() {
        info!(moderation_id = %a.id, attempt_id = %a.exam_attempt_id, "affected moderation record");
    }

    let confirmed = std::env::var("CONFIRM").is_ok_and(|v| v == "1");
    if !confirmed {
        println!(
            "DRY RUN: {} moderation records would be reset. Set CONFIRM=1 to apply.",
            affected.len()
        );
        return Ok(());
    }

    // Update by the exact ids collected above, so the write cannot pick up records created by a
    // concurrent moderation-service run between the read and the write.
    let ids = affected.iter().map(|a| a.id).collect::<Vec<_>>();
    let update_result = moderation_col
        .update_many(
            doc! {"_id": {"$in": &ids}},
            doc! {"$set": {"challengesAwarded": false}},
        )
        .await
        .map_err(err("unable to reset challengesAwarded"))?;

    if update_result.modified_count != affected.len() as u64 {
        warn!(
            found = affected.len(),
            modified = update_result.modified_count,
            "modified count does not match found count"
        );
    }

    println!(
        "Reset challengesAwarded on {} moderation records.",
        update_result.modified_count
    );
    println!("Run the moderation service `award_challenge_ids` task to award the challenges.");

    Ok(())
}

fn err<E>(s: &str) -> impl FnOnce(E) -> String
where
    E: ToString,
{
    return move |e: E| format!("{}: {}", s, e.to_string());
}
