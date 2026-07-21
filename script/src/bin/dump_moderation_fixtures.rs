//! Dump moderation-score evaluation fixtures.
//!
//! Fetches the last 200 non-`Pending` `ExamEnvironmentExamModeration` records from
//! MongoDB, and for each the matching attempt / exam / generated exam, plus the
//! attempt's events from Supabase. Extracts the previously-calculated moderation
//! score (`prevModerationScore`) from the moderation `feedback` field via
//! `/\d\.\d+$/`, falling back to the stored `moderationScore` field, and writes
//! everything under the git-ignored `fixtures/` dir for the `exam-utils`
//! moderation-score diff test to consume.
//!
//! Env: `MONGODB_URI`, `SUPABASE_URL`, `SUPABASE_KEY`
//! Run: `cargo run --bin dump_moderation_fixtures`

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use futures_util::TryStreamExt;
use mongodb::bson::{doc, oid::ObjectId};
use prisma::{
    ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentExamModeration,
    ExamEnvironmentExamModerationStatus, ExamEnvironmentGeneratedExam, db::*, supabase::Event,
};
use regex::Regex;
use serde::Serialize;
use supabase_rs::SupabaseClient;
use tracing::{info, warn};

/// Most-recent non-Pending moderation records to sample.
const LIMIT: i64 = 200;
/// Workspace-root `fixtures/` dir (resolved from crate manifest, CWD-independent).
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(format!("{}=info", env!("CARGO_CRATE_NAME")))
        .init();
    dotenvy::dotenv().ok();

    let mongo_uri = std::env::var("MONGODB_URI").context("MONGODB_URI not set")?;
    let supabase_url = std::env::var("SUPABASE_URL").context("SUPABASE_URL not set")?;
    let supabase_key = std::env::var("SUPABASE_KEY").context("SUPABASE_KEY not set")?;

    let client = client(&mongo_uri).await?;
    let supabase = SupabaseClient::new(&supabase_url, &supabase_key).map_err(anyhow::Error::msg)?;

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

    for dir in ["moderation", "attempt", "exam", "generation", "events"] {
        std::fs::create_dir_all(Path::new(FIXTURES).join(dir))
            .with_context(|| format!("unable to create fixtures/{dir}"))?;
    }

    // Last `LIMIT` moderated (non-Pending) records, newest first by `_id`.
    let moderations: Vec<ExamEnvironmentExamModeration> = moderation_collection
        .find(doc! { "status": { "$ne": ExamEnvironmentExamModerationStatus::Pending } })
        .sort(doc! { "_id": -1 })
        .limit(LIMIT)
        .await?
        .try_collect()
        .await
        .context("unable to fetch moderation records")?;
    info!(count = moderations.len(), "fetched moderation records");

    // Trailing decimal in feedback, e.g. "Moderation score: 0.3421" -> 0.3421.
    let score_re = Regex::new(r"\d\.\d+$").expect("static regex is valid");

    let mut written_exams: HashSet<ObjectId> = HashSet::new();
    let mut written_generations: HashSet<ObjectId> = HashSet::new();

    let mut written = 0usize;
    let mut missing_attempt = 0usize;
    let mut with_prev = 0usize;

    for moderation in &moderations {
        let attempt_id = moderation.exam_attempt_id;

        let Some(attempt) = attempt_collection
            .find_one(doc! { "_id": attempt_id })
            .await
            .context("attempt query failed")?
        else {
            warn!(attempt = %attempt_id, "no attempt for moderation record");
            missing_attempt += 1;
            continue;
        };

        // Exam + generation are shared across attempts; fetch + write once each.
        if written_exams.insert(attempt.exam_id) {
            let exam = exam_collection
                .find_one(doc! { "_id": attempt.exam_id })
                .await?
                .context("unable to find exam for attempt")?;
            write_json(&fixture_path("exam", &attempt.exam_id), &exam)?;
        }
        if written_generations.insert(attempt.generated_exam_id) {
            let generation = generation_collection
                .find_one(doc! { "_id": attempt.generated_exam_id })
                .await?
                .context("unable to find generated exam for attempt")?;
            write_json(
                &fixture_path("generation", &attempt.generated_exam_id),
                &generation,
            )?;
        }

        let events = get_events_for_attempt(&supabase, &attempt_id).await?;

        // `prevModerationScore`: prefer the score embedded in feedback (above-threshold
        // records), fall back to the stored field (below-threshold auto-approvals).
        let prev_from_feedback = moderation
            .feedback
            .as_deref()
            .and_then(|f| score_re.find(f.trim()))
            .and_then(|m| m.as_str().parse::<f64>().ok());
        let prev_moderation_score = prev_from_feedback.or(moderation.moderation_score);
        if prev_moderation_score.is_some() {
            with_prev += 1;
        }

        // Serialize the moderation record and inject the derived prev score.
        let mut moderation_json = serde_json::to_value(moderation)?;
        moderation_json
            .as_object_mut()
            .context("moderation record did not serialize to an object")?
            .insert(
                "prevModerationScore".to_string(),
                serde_json::to_value(prev_moderation_score)?,
            );

        // Moderation keyed by attempt id so the test can join records trivially.
        write_json(&fixture_path("moderation", &attempt_id), &moderation_json)?;
        write_json(&fixture_path("attempt", &attempt_id), &attempt)?;
        write_json(&fixture_path("events", &attempt_id), &events)?;
        written += 1;
    }

    info!(
        written,
        with_prev,
        missing_attempt,
        exams = written_exams.len(),
        generations = written_generations.len(),
        dir = FIXTURES,
        "done"
    );
    Ok(())
}

fn fixture_path(dir: &str, id: &ObjectId) -> PathBuf {
    Path::new(FIXTURES).join(dir).join(id.to_hex())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    std::fs::write(path, bytes).with_context(|| format!("unable to write {}", path.display()))?;
    Ok(())
}

/// Mirrors `moderation-service`'s Supabase event fetch: deserialize into the exact
/// `Event` type the scorer consumes so the written fixture round-trips cleanly.
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
        .context("unable to get events for attempt")?;

    let events: Vec<Event> = events
        .into_iter()
        .filter_map(|event| match serde_json::from_value(event) {
            Ok(event) => Some(event),
            Err(e) => {
                warn!(error = ?e, attempt = %attempt_id, "unable to deserialize event");
                None
            }
        })
        .collect();
    Ok(events)
}
