//! Generate `prevModerationScore` fixtures from the *previous* moderation-score
//! algorithm, so the `exam-utils` `moderation_score_diff` test can measure how the
//! current algorithm diverges from it.
//!
//! Unlike `dump_moderation_fixtures` (which pulls prod scores out of Mongo/Supabase),
//! this needs no DB: it reads the production data already sitting in `fixtures/`
//! (attempt / exam / generation / events) and recomputes each attempt's score with
//! the OLD `get_moderation_score`, copied verbatim below from
//! `exam-utils/src/attempt.rs` at commit `bc6af64^` (i.e. before `bc6af64
//! "fix: update deps and schema"` changed the algorithm).
//!
//! Keeping the old function's source here lets the two algorithm versions be
//! diffed/tracked side by side. Output: `fixtures/moderation/<attemptId>`.
//!
//! Run: `cargo run --bin generate_prev_moderation_scores` (in `../script`)

use std::path::{Path, PathBuf};

use exam_utils::attempt::{Attempt, construct_attempt};
use exam_utils::error::Error;
use mongodb::bson::oid::ObjectId;
use prisma::{
    ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentGeneratedExam,
    supabase::{Event, EventKind},
};
use serde_json::json;

/// Workspace-root `fixtures/` dir (resolved from crate manifest, CWD-independent).
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures");

fn main() -> anyhow::Result<()> {
    let moderation_dir = Path::new(FIXTURES).join("moderation");
    std::fs::create_dir_all(&moderation_dir)?;

    let mut written = 0usize;
    let mut errored = 0usize;
    let mut missing = 0usize;

    for entry in std::fs::read_dir(Path::new(FIXTURES).join("attempt"))? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }

        let attempt: ExamEnvironmentExamAttempt = serde_json::from_slice(&std::fs::read(&path)?)?;

        // Skip attempts whose supporting fixtures are absent.
        if !fixture_exists("exam", &attempt.exam_id)
            || !fixture_exists("generation", &attempt.generated_exam_id)
            || !fixture_exists("events", &attempt.id)
        {
            missing += 1;
            continue;
        }

        let exam: ExamEnvironmentExam =
            serde_json::from_slice(&std::fs::read(fixture_path("exam", &attempt.exam_id))?)?;
        let generation: ExamEnvironmentGeneratedExam = serde_json::from_slice(&std::fs::read(
            fixture_path("generation", &attempt.generated_exam_id),
        )?)?;
        let events: Vec<Event> =
            serde_json::from_slice(&std::fs::read(fixture_path("events", &attempt.id))?)?;

        let constructed = construct_attempt(&exam, &generation, &attempt);

        match get_moderation_score_pre_bc6af64(&constructed, &events) {
            Ok(prev_moderation_score) => {
                let record = json!({
                    "examAttemptId": attempt.id.to_hex(),
                    "prevModerationScore": prev_moderation_score,
                    // Provenance so the value is not mistaken for a prod score.
                    "prevModerationScoreSource": "get_moderation_score@bc6af64^",
                });
                std::fs::write(
                    fixture_path("moderation", &attempt.id),
                    serde_json::to_vec_pretty(&record)?,
                )?;
                written += 1;
            }
            Err(e) => {
                errored += 1;
                eprintln!("skip {}: {e}", attempt.id.to_hex());
            }
        }
    }

    println!(
        "wrote {written} prev-score fixtures ({errored} algo errors, {missing} missing supporting fixtures) -> {}",
        moderation_dir.display()
    );
    Ok(())
}

fn fixture_path(dir: &str, id: &ObjectId) -> PathBuf {
    Path::new(FIXTURES).join(dir).join(id.to_hex())
}

fn fixture_exists(dir: &str, id: &ObjectId) -> bool {
    fixture_path(dir, id).exists()
}

// ============================================================================
// PREVIOUS ALGORITHM - copied verbatim from exam-utils/src/attempt.rs @ bc6af64^
// (before commit bc6af64 "fix: update deps and schema").
//
// Diff vs current: current comments out `blur_weight` (total_blur_time / total_time)
// and the `total_blur_time_before_last_answer > total_time_taken` guard. Do NOT
// "fix" or refactor this - it must stay frozen at the old behaviour.
// ============================================================================
fn get_moderation_score_pre_bc6af64(attempt: &Attempt, events: &Vec<Event>) -> Result<f64, Error> {
    // (1 / number of parts)
    let weight = 0.25;
    let mut moderation_score = 0.0;
    let mut total_blur_time = 0.0;
    let mut total_blur_time_before_last_answer = 0.0;

    let mut events = events.clone();
    events.sort_by(|a, b| (a.timestamp).cmp(&b.timestamp));

    let last_submission_time = attempt
        .question_sets
        .iter()
        .flat_map(|qs| qs.questions.iter().flat_map(|q| q.submission_time))
        .max();

    let last_submission_time = match last_submission_time {
        Some(last_submission_time) => last_submission_time,
        None => {
            // Theoretically, this should be impossible -> function currently only called if attempt passes
            tracing::warn!(attempt = %attempt.id, "attempt did not submit any answers");
            return Ok(moderation_score);
        }
    };

    let mut previous_blur_time = None;
    for event in events {
        let timestamp = event.timestamp;
        match event.kind {
            EventKind::Blur => {
                previous_blur_time = Some(timestamp);
            }
            EventKind::Focus => {
                if let Some(previous_blur_time) = previous_blur_time {
                    let blur_time = (timestamp - previous_blur_time).as_seconds_f64();
                    total_blur_time += blur_time;

                    if timestamp.timestamp_millis() < last_submission_time.timestamp_millis() {
                        total_blur_time_before_last_answer += blur_time;
                    }
                }
            }

            _ => {}
        }
    }

    // Time taken to answer all questions -> does not include checking over answers / waiting before exiting
    let total_time_taken = last_submission_time
        .saturating_duration_since(attempt.start_time)
        .as_secs_f64();

    let total_time = attempt.config.total_time_in_s as f64;

    if total_time_taken > total_time {
        return Err(Error::ModerationScore(format!(
            "total time taken > than total time: {} > {}",
            total_time_taken, total_time
        )));
    }
    if total_blur_time > total_time {
        return Err(Error::ModerationScore(format!(
            "total blur time > total time: {total_blur_time} > {total_time}"
        )));
    }
    if total_blur_time_before_last_answer > total_blur_time {
        return Err(Error::ModerationScore(format!(
            "total blur time before last answer > total blur time: {total_blur_time_before_last_answer} > {total_blur_time}"
        )));
    }

    let time_weight = ((total_time - total_time_taken) / total_time) * weight;
    if time_weight > weight {
        return Err(Error::ModerationScore(format!(
            "time weight > weight: {time_weight} > {weight}"
        )));
    }
    moderation_score += time_weight;

    // Blur time after last submission is worth 1/3 as much as before last submission
    // Seeing as both total_blur_time_* vars include the time before, it is counted 'twice'
    let blur_weight = (total_blur_time / total_time) * weight;
    if blur_weight > weight {
        return Err(Error::ModerationScore(format!(
            "blur weight > weight: {blur_weight} > {weight}"
        )));
    }
    moderation_score += blur_weight;

    if total_blur_time_before_last_answer > total_time_taken {
        return Err(Error::ModerationScore(format!(
            "total blur time before last answer > total time taken: {total_blur_time_before_last_answer} > {total_time_taken}"
        )));
    }

    let blur_before_weight = (total_blur_time_before_last_answer / total_time_taken) * weight * 2.0;
    if blur_before_weight > weight * 2.0 {
        return Err(Error::ModerationScore(format!(
            "blur before weight > weight: {blur_before_weight} > {}",
            weight * 2.0
        )));
    }
    moderation_score += blur_before_weight;

    if moderation_score > 1.0 {
        tracing::error!(
            attempt = %attempt.id,
            moderation_score,
            "moderation score should never be > 1.0"
        );
    }

    Ok(moderation_score)
}
