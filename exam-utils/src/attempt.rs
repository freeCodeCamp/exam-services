use bson::oid::ObjectId;
use prisma::{
    self,
    supabase::{Event, EventKind},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

use crate::error::Error;

pub fn get_time_between_submissions(_attempt: prisma::ExamEnvironmentExamAttempt) -> Vec<Duration> {
    todo!()
}

#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub id: ObjectId,
    #[serde(rename = "examId")]
    pub exam_id: ObjectId,
    #[serde(rename = "userId")]
    pub user_id: ObjectId,
    pub prerequisites: Vec<ObjectId>,
    pub deprecated: bool,
    #[serde(rename = "questionSets")]
    pub question_sets: Vec<AttemptQuestionSet>,
    pub config: prisma::ExamEnvironmentConfig,
    #[serde(rename = "startTime")]
    #[serde_as(as = "bson::serde_helpers::datetime::AsRfc3339String")]
    pub start_time: bson::DateTime,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttemptQuestionSet {
    pub id: ObjectId,
    #[serde(rename = "type")]
    pub _type: prisma::ExamEnvironmentQuestionType,
    pub context: Option<String>,
    pub questions: Vec<AttemptQuestionSetQuestion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttemptQuestionSetQuestion {
    pub id: ObjectId,
    pub text: String,
    pub tags: Vec<String>,
    pub deprecated: bool,
    pub audio: Option<prisma::ExamEnvironmentAudio>,
    /// Includes all answers available in the exam
    pub answers: Vec<prisma::ExamEnvironmentAnswer>,
    /// Includes only answers submitted in the attempt
    pub selected: Vec<ObjectId>,
    /// Includes only answers shown from the generation
    pub generated: Vec<ObjectId>,
    /// If question was submitted, time it was submitted
    #[serde(rename = "submissionTime")]
    pub submission_time: Option<bson::DateTime>,
}

/// Constructs an `Attempt`:
/// - Filters questions from exam based on generated exam
/// - Adds submission time from attempt questions
/// - Adds selected answers from attempt
///
/// NOTE: Generated exam is assumed to not be needed,
/// because API ensures attempt only includes answers from assigned generation.
pub fn construct_attempt(
    exam: &prisma::ExamEnvironmentExam,
    generation: &prisma::ExamEnvironmentGeneratedExam,
    exam_attempt: &prisma::ExamEnvironmentExamAttempt,
) -> Attempt {
    let prisma::ExamEnvironmentExam {
        id: _id,
        question_sets,
        config,
        prerequisites,
        deprecated,
        version: _version,
    } = exam;
    // TODO: Can caluclate allocation size from exam
    let mut attempt_question_sets = vec![];

    for question_set in question_sets {
        let prisma::ExamEnvironmentQuestionSet {
            id,
            _type,
            context,
            questions,
        } = question_set;

        // Attempt might not have question set, if related question(s) not answered
        let attempt_question_set = exam_attempt
            .question_sets
            .iter()
            .find(|qs| qs.id == question_set.id);
        let generation_question_set = generation
            .question_sets
            .iter()
            .find(|qs| qs.id == question_set.id);

        let mut attempt_questions = vec![];

        for question in questions {
            let prisma::ExamEnvironmentMultipleChoiceQuestion {
                id,
                text,
                tags,
                audio,
                answers,
                deprecated,
            } = question;

            let mut selected = vec![];
            let mut generated = vec![];
            let mut submission_time = None;
            // Attempt question might not exist if not answered
            if let Some(aqs) = attempt_question_set {
                if let Some(aq) = aqs.questions.iter().find(|q| q.id == *id) {
                    selected.extend_from_slice(&aq.answers);
                    submission_time = Some(aq.submission_time);
                };
            }

            // TODO: It should be impossible for the generation question set to not exist if the attempt encountered it
            if let Some(gqs) = generation_question_set {
                if let Some(gq) = gqs.questions.iter().find(|q| q.id == *id) {
                    generated.extend_from_slice(&gq.answers);
                }
            }

            let attempt_question_set_question = AttemptQuestionSetQuestion {
                id: id.clone(),
                text: text.clone(),
                tags: tags.clone(),
                deprecated: deprecated.clone(),
                audio: audio.clone(),
                answers: answers.clone(),
                selected,
                generated,
                submission_time,
            };

            attempt_questions.push(attempt_question_set_question);
        }

        let attempt_question_set = AttemptQuestionSet {
            id: id.clone(),
            _type: _type.clone(),
            context: context.clone(),
            questions: attempt_questions,
        };

        attempt_question_sets.push(attempt_question_set);
    }

    let start_time = exam_attempt.start_time;

    let attempt = Attempt {
        id: exam_attempt.id,
        exam_id: exam_attempt.exam_id,
        user_id: exam_attempt.user_id,
        prerequisites: prerequisites.clone(),
        deprecated: *deprecated,
        question_sets: attempt_question_sets,
        config: config.clone(),
        start_time,
    };

    attempt
}

pub struct AttemptStats {
    pub time_to_answers: Vec<TimeToAnswer>,
    pub total_questions: usize,
    pub answered: usize,
    pub correct: usize,
    pub time_to_complete: f64,
    pub average_time_per_question: f64,
}

pub struct TimeToAnswer {
    pub name: usize,
    pub value: f64,
    pub is_correct: bool,
}

pub struct QuestionBlurPeriods {
    pub question_id: ObjectId,
    pub periods: Vec<f64>,
}

pub fn get_attempt_stats(_attempt: Attempt) -> AttemptStats {
    todo!()
}

/// Calculates a 0.0 -> 1.0 score.
///
/// - A score of 0.0 means the attempt definitely does **not** need moderation.
/// - A score of 1.0 means the attempt definitely does need moderation.
///
///
pub fn get_moderation_score(attempt: &Attempt, events: &Vec<Event>) -> Result<f64, Error> {
    // (1 / number of parts)
    let weight = 0.3333;
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

    let mut blur_periods: Vec<QuestionBlurPeriods> = vec![];
    let mut pending_blur: HashMap<ObjectId, _> = HashMap::new();
    for event in events {
        let timestamp = event.timestamp;
        let Some(question_id) = event
            .meta
            .get("question")
            .and_then(|v| v.as_str())
            .and_then(|s| ObjectId::parse_str(s).ok())
        else {
            continue;
        };

        match event.kind {
            EventKind::Blur => {
                pending_blur.entry(question_id).or_insert(timestamp);
            }
            EventKind::Focus => {
                if let Some(blur_start) = pending_blur.remove(&question_id) {
                    let blur_time = (timestamp - blur_start).as_seconds_f64();
                    total_blur_time += blur_time;

                    if timestamp.timestamp_millis() < last_submission_time.timestamp_millis() {
                        total_blur_time_before_last_answer += blur_time;
                    }

                    match blur_periods
                        .iter_mut()
                        .find(|q| q.question_id == question_id)
                    {
                        Some(q) => q.periods.push(blur_time),
                        None => blur_periods.push(QuestionBlurPeriods {
                            question_id,
                            periods: vec![blur_time],
                        }),
                    }
                }
            }
            _ => {}
        }
    }

    if !pending_blur.is_empty() {
        return Err(Error::ModerationScore(format!(
            "{} blurs with no matching re-focus event: \n{:?}\n{:?}",
            pending_blur.len(),
            pending_blur,
            last_submission_time
        )));
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

#[cfg(test)]
mod tests {
    use std::f64;

    use bson::oid::ObjectId;
    use prisma::{
        ExamEnvironmentExam, ExamEnvironmentExamAttempt, ExamEnvironmentGeneratedExam,
        supabase::Event,
    };

    use crate::attempt::{construct_attempt, get_moderation_score};

    fn get_events_for_attempt(attempt_id: &ObjectId) -> Vec<Event> {
        let event = std::fs::read(format!("../fixtures/events/{}", attempt_id.to_hex())).unwrap();

        let events: Vec<Event> = serde_json::from_slice(&event).unwrap();
        events
    }

    fn get_attempts() -> Vec<ExamEnvironmentExamAttempt> {
        let attempts_dir = std::fs::read_dir("../fixtures/attempt").unwrap();

        let mut attempts = vec![];
        for f in attempts_dir {
            let attempt = std::fs::read(f.unwrap().path()).unwrap();
            let attempt: ExamEnvironmentExamAttempt = serde_json::from_slice(&attempt).unwrap();
            attempts.push(attempt);
        }

        attempts
    }

    fn get_exam_by_id(exam_id: &ObjectId) -> ExamEnvironmentExam {
        let exam = std::fs::read(format!("../fixtures/exam/{}", exam_id.to_hex())).unwrap();

        let exam = serde_json::from_slice(&exam).unwrap();
        exam
    }

    fn get_generation_by_id(generation_id: &ObjectId) -> ExamEnvironmentGeneratedExam {
        let f =
            std::fs::read(format!("../fixtures/generation/{}", generation_id.to_hex())).unwrap();

        let x = serde_json::from_slice(&f).unwrap();
        x
    }

    #[test]
    fn moderation_score() {
        let attempts = get_attempts();

        let mut scores = vec![];
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for attempt in attempts {
            let exam = get_exam_by_id(&attempt.exam_id);
            let generation = get_generation_by_id(&attempt.generated_exam_id);
            let events = get_events_for_attempt(&attempt.id);

            let attempt = construct_attempt(&exam, &generation, &attempt);

            let score = get_moderation_score(&attempt, &events)
                .map_err(|e| format!("{}: {e}", attempt.id))
                .unwrap();

            if score < min {
                min = score;
            }
            if score > max {
                max = score;
            }

            scores.push(format!("{:.3}", score));
        }

        println!("{:#?}", scores);

        dbg!(min, max);

        assert!(max <= 1.0);
        assert!(min >= 0.0);
    }

    /// Moderation threshold: at/above => flagged for manual moderation.
    /// Mirrors `moderation-service` `MODERATION_THRESHOLD` default.
    const THRESHOLD: f64 = 0.25;

    fn get_attempt_by_id(attempt_id: &ObjectId) -> ExamEnvironmentExamAttempt {
        let attempt =
            std::fs::read(format!("../fixtures/attempt/{}", attempt_id.to_hex())).unwrap();
        serde_json::from_slice(&attempt).unwrap()
    }

    struct DiffRow {
        attempt_id: ObjectId,
        prev: f64,
        new: f64,
    }

    impl DiffRow {
        fn delta(&self) -> f64 {
            self.new - self.prev
        }
        /// Threshold band changed between prev and new.
        fn crossed(&self) -> Option<&'static str> {
            match (self.prev >= THRESHOLD, self.new >= THRESHOLD) {
                (false, true) => Some("NOW-FLAGGED"),
                (true, false) => Some("NOW-CLEARED"),
                _ => None,
            }
        }
    }

    /// Recomputes `get_moderation_score` for every dumped moderation fixture and
    /// diffs it against the previously-stored score (`prevModerationScore`).
    ///
    /// Not really a test - no meaningful assertions. Uses the `#[test]` harness
    /// purely so it can be run with the crate deps wired up. Populate fixtures
    /// via `cargo run --bin dump_moderation_fixtures` in `../script`, then:
    ///   `cargo test -p exam-utils moderation_score_diff -- --nocapture`
    #[test]
    fn moderation_score_diff() {
        #[derive(serde::Deserialize)]
        struct ModerationFixture {
            #[serde(rename = "prevModerationScore")]
            prev: Option<f64>,
        }

        let dir = match std::fs::read_dir("../fixtures/moderation") {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!(
                    "no ../fixtures/moderation ({e}). Run: cargo run --bin dump_moderation_fixtures"
                );
                return;
            }
        };

        let mut rows = vec![];
        let mut skipped_no_prev = 0usize;
        let mut recompute_errors = 0usize;
        let mut missing_fixtures = 0usize;

        for entry in dir {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            let Some(attempt_id) = path
                .file_name()
                .and_then(|f| f.to_str())
                .and_then(|f| ObjectId::parse_str(f).ok())
            else {
                continue;
            };

            let moderation: ModerationFixture =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let Some(prev) = moderation.prev else {
                skipped_no_prev += 1;
                continue;
            };

            let attempt = get_attempt_by_id(&attempt_id);
            // Fixtures for exam/generation/events may be absent for stale records.
            let exists = |dir: &str, id: &ObjectId| {
                std::path::Path::new(&format!("../fixtures/{dir}/{}", id.to_hex())).exists()
            };
            if !exists("exam", &attempt.exam_id)
                || !exists("generation", &attempt.generated_exam_id)
                || !exists("events", &attempt.id)
            {
                missing_fixtures += 1;
                continue;
            }

            let exam = get_exam_by_id(&attempt.exam_id);
            let generation = get_generation_by_id(&attempt.generated_exam_id);
            let events = get_events_for_attempt(&attempt.id);

            let constructed = construct_attempt(&exam, &generation, &attempt);
            match get_moderation_score(&constructed, &events) {
                Ok(new) => rows.push(DiffRow {
                    attempt_id,
                    prev,
                    new,
                }),
                Err(e) => {
                    recompute_errors += 1;
                    eprintln!("recompute error {attempt_id}: {e}");
                }
            }
        }

        rows.sort_by(|a, b| b.delta().abs().partial_cmp(&a.delta().abs()).unwrap());
        print_report(&rows, skipped_no_prev, recompute_errors, missing_fixtures);
    }

    fn print_report(
        rows: &[DiffRow],
        skipped_no_prev: usize,
        recompute_errors: usize,
        missing_fixtures: usize,
    ) {
        let n = rows.len();
        println!("\n╔══════════════════════════════════════════════════════════════════════╗");
        println!("║           MODERATION SCORE DIFF  (new algorithm vs previous)           ║");
        println!("╚══════════════════════════════════════════════════════════════════════╝");

        if n == 0 {
            println!(
                "\n  no comparable records (skipped_no_prev={skipped_no_prev}, recompute_errors={recompute_errors}, missing_fixtures={missing_fixtures})\n"
            );
            return;
        }

        println!(
            "\n  {:<26} {:>8} {:>8} {:>9}  {}",
            "attempt", "prev", "new", "Δ", "crossed"
        );
        println!("  {}", "─".repeat(70));
        for r in rows {
            println!(
                "  {:<26} {:>8.4} {:>8.4} {:>+9.4}  {}",
                r.attempt_id.to_hex(),
                r.prev,
                r.new,
                r.delta(),
                r.crossed().unwrap_or("")
            );
        }

        // Aggregate stats.
        let deltas: Vec<f64> = rows.iter().map(|r| r.delta()).collect();
        let abs: Vec<f64> = deltas.iter().map(|d| d.abs()).collect();
        let sum: f64 = deltas.iter().sum();
        let abs_sum: f64 = abs.iter().sum();
        let mean = sum / n as f64;
        let abs_mean = abs_sum / n as f64;
        let variance = deltas.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n as f64;
        let stddev = variance.sqrt();
        let max = deltas.iter().cloned().fold(f64::MIN, f64::max);
        let min = deltas.iter().cloned().fold(f64::MAX, f64::min);

        let mut sorted_abs = abs.clone();
        sorted_abs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_abs = sorted_abs[n / 2];

        let eps = 1e-9;
        let increased = deltas.iter().filter(|d| **d > eps).count();
        let decreased = deltas.iter().filter(|d| **d < -eps).count();
        let unchanged = n - increased - decreased;

        let now_flagged = rows
            .iter()
            .filter(|r| r.crossed() == Some("NOW-FLAGGED"))
            .count();
        let now_cleared = rows
            .iter()
            .filter(|r| r.crossed() == Some("NOW-CLEARED"))
            .count();

        println!("\n  {}", "─".repeat(70));
        println!("  SUMMARY");
        println!("  {}", "─".repeat(70));
        println!("  compared          {n}");
        println!("  skipped (no prev) {skipped_no_prev}");
        println!("  recompute errors  {recompute_errors}");
        println!("  missing fixtures  {missing_fixtures}");
        println!();
        println!("  Δ mean            {mean:+.4}");
        println!("  Δ stddev          {stddev:.4}");
        println!("  Δ min / max       {min:+.4} / {max:+.4}");
        println!("  |Δ| mean          {abs_mean:.4}");
        println!("  |Δ| median        {median_abs:.4}");
        println!();
        println!(
            "  increased         {increased}  ({:.0}%)",
            100.0 * increased as f64 / n as f64
        );
        println!(
            "  decreased         {decreased}  ({:.0}%)",
            100.0 * decreased as f64 / n as f64
        );
        println!(
            "  unchanged         {unchanged}  ({:.0}%)",
            100.0 * unchanged as f64 / n as f64
        );
        println!();
        println!("  threshold ({THRESHOLD})");
        println!("    newly flagged   {now_flagged}  (prev < T, new ≥ T)");
        println!("    newly cleared   {now_cleared}  (prev ≥ T, new < T)");

        // |Δ| distribution. Bars scaled so the fullest bucket is 50 wide.
        // let buckets = [0.0, 0.01, 0.05, 0.10, 0.25, 0.50, 1.01];
        let num_buckets = 100;
        let buckets: Vec<f64> = (0..=num_buckets)
            .map(|i| i as f64 * (1.0 / num_buckets as f64))
            .collect();
        let counts: Vec<usize> = buckets
            .windows(2)
            .map(|w| abs.iter().filter(|d| **d >= w[0] && **d < w[1]).count())
            .collect();
        let peak = counts.iter().copied().max().unwrap_or(0).max(1);
        println!("\n  |Δ| distribution");
        for (w, &count) in buckets.windows(2).zip(&counts) {
            if count == 0 {
                continue;
            }
            let (lo, hi) = (w[0], w[1]);
            let bar = "█".repeat(count * 50 / peak);
            println!("    [{lo:.2}, {hi:.2})  {count:>4}  {bar}");
        }
        println!();
    }
}
