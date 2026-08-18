//! Generate `prevModerationScore` fixtures from moderation-score algorithm `v1`,
//! so the `exam-utils` `moderation_score_diff` test can measure how the live
//! algorithm diverges from it.
//!
//! Unlike `dump_moderation_fixtures` (which pulls prod scores out of Mongo/Supabase),
//! this needs no DB: it reads the production data already sitting in `fixtures/`
//! (attempt / exam / generation / events) and recomputes each attempt's score with
//! `v1` alongside the live version.
//!
//! Both algorithms come from the `exam_utils::moderation_versions` registry
//! (single source of truth, also used by the `exam-utils` comparison harness);
//! each written score records the stable version label that produced it. Output:
//! `fixtures/moderation/<attemptId>`.
//!
//! Run: `cargo run --bin generate_prev_moderation_scores` (in `../script`)

use std::path::{Path, PathBuf};

use exam_utils::attempt::construct_attempt;
use exam_utils::moderation_versions::{self, by_id};
use mongodb::bson::oid::ObjectId;
use prisma::{
    ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentGeneratedExam,
    supabase::Event,
};
use serde_json::json;

/// Workspace-root `fixtures/` dir (resolved from crate manifest, CWD-independent).
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures");

/// Baseline version the `prevModerationScore` column is generated from.
const PREV: u32 = 1;

fn main() -> anyhow::Result<()> {
    let prev = by_id(PREV).expect("baseline version must be registered");
    let live = moderation_versions::live();

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

        match (prev.score)(&constructed, &events) {
            Ok(prev_moderation_score) => {
                let moderation_score = match (live.score)(&constructed, &events) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        eprintln!("{}-score skip {}: {e}", live.label, attempt.id.to_hex());
                        None
                    }
                };
                let record = json!({
                    "examAttemptId": attempt.id.to_hex(),
                    "prevModerationScore": prev_moderation_score,
                    // Stable version label, so the value is neither mistaken for a
                    // prod score nor for the output of a later algorithm.
                    "prevModerationScoreVersion": prev.label,
                    "moderationScore": moderation_score,
                    "moderationScoreVersion": live.label,
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
