use bson::{DateTime, oid::ObjectId};
use prisma::{
    self,
    supabase::{Event, EventKind},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    pub periods: Vec<Period>,
}

#[derive(Clone)]
pub struct Period {
    pub start: DateTime,
    pub end: DateTime,
}

pub fn get_attempt_stats(_attempt: Attempt) -> AttemptStats {
    todo!()
}

/// Calculates a 0.0 -> 1.0 score.
///
/// - A score of 0.0 means the attempt definitely does **not** need moderation.
/// - A score of 1.0 means the attempt definitely does need moderation.
pub fn get_moderation_score(attempt: &Attempt, events: &Vec<Event>) -> Result<f64, Error> {
    let weight = 1.0 / 4.0;
    let mut moderation_score = 0.0;

    let any_answered = attempt
        .question_sets
        .iter()
        .flat_map(|qs| qs.questions.iter())
        .any(|q| q.submission_time.is_some());

    if !any_answered {
        // Theoretically impossible -> function currently only called if attempt passes
        tracing::warn!(attempt = %attempt.id, "attempt did not submit any answers");
        return Ok(moderation_score);
    }

    let mut events = events.clone();
    events.sort_by(|a, b| (a.timestamp).cmp(&b.timestamp));

    let last_submission_time = get_last_submission_time(&attempt);

    let total_number_of_questions = attempt
        .question_sets
        .iter()
        .flat_map(|qs| qs.questions.iter())
        .filter(|q| q.submission_time.is_some())
        .count();

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

    let time_taken_percent = total_time_taken / total_time;
    // map 10% -> 0 score; 0% -> 0.25 score
    if time_taken_percent < 0.1 {
        moderation_score += ((time_taken_percent - 0.1).abs() / 0.1) * weight;
    }

    // Median Time Taken to Answer Per Question
    let mut time_per_question = get_time_per_question(attempt, &events);
    time_per_question.sort();
    let median_time_per_question = time_per_question
        .median()
        .ok_or(Error::ModerationScore(format!(
            "unable to calculate median for time per question"
        )))?
        .as_secs_f64();
    // map 5s -> 0 score; 0s -> 0.25 score
    if median_time_per_question < 5.0 {
        moderation_score += ((median_time_per_question - 5.0).abs() / 5.0) * weight;
    }

    // Number of Questions Blurred
    let blur_periods = get_blur_periods(&events);
    let blur_periods_before = blur_periods
        .iter()
        .filter_map(|bp| {
            let periods = bp.periods.clone();
            let periods_before = periods
                .into_iter()
                .filter(|p| p.start < last_submission_time)
                .collect::<Vec<_>>();
            if periods_before.is_empty() {
                return None;
            }

            Some(QuestionBlurPeriods {
                question_id: bp.question_id,
                periods: periods_before,
            })
        })
        .collect::<Vec<_>>();
    let num_questions_blurred_before = blur_periods_before.len();

    // adds 0.25 to score, if >= 20% of questions
    let questions_blurred_percent =
        num_questions_blurred_before as f64 / total_number_of_questions as f64;
    let questions_blurred_weight =
        (questions_blurred_percent * 100.0 / (100.0 - attempt.config.passing_percent)).min(1.0)
            * weight;
    moderation_score += questions_blurred_weight;

    if moderation_score > 1.0 {
        tracing::error!(
            attempt = %attempt.id,
            moderation_score,
            "moderation score should never be > 1.0"
        );
    }

    Ok(moderation_score)
}

trait Median<T> {
    fn median(&self) -> Option<T>;
}

impl<T: Ord + Copy + std::ops::Add<Output = T> + std::ops::Div<u32, Output = T>> Median<T>
    for [T]
{
    fn median(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let mut sorted = self.to_vec();
        sorted.sort();
        let midpoint = sorted.len() / 2;
        Some(if sorted.len() % 2 == 0 {
            (sorted[midpoint - 1] + sorted[midpoint]) / 2
        } else {
            sorted[midpoint]
        })
    }
}

/// Time spent per question before final submission.
///
/// Time on question = question.submission_time - timestamp of the latest
/// `QuestionVisit` event for that question at or before the submission.
///
/// The attempt bounds the span, so - unlike pairing consecutive visits - the
/// last question shown is not dropped, and time spent revisiting a question
/// after it was answered is not counted.
///
/// Fault-tolerant to missing visit events: with no visit for a question, the
/// span starts at the nearest earlier submission of any other question -
/// questions are not necessarily answered in order, so this is the latest
/// submission before this one, not the preceding question in the exam. Falling
/// back further, the attempt's start time is used.
///
/// Questions with no submission are omitted.
pub fn get_time_per_question(attempt: &Attempt, events: &Vec<Event>) -> Vec<Duration> {
    let mut visits: Vec<(ObjectId, DateTime)> = vec![];

    for event in events {
        if !matches!(event.kind, EventKind::QuestionVisit) {
            continue;
        }
        let Some(question_id) = event
            .meta
            .get("question")
            .and_then(|v| v.as_str())
            .and_then(|s| ObjectId::parse_str(s).ok())
        else {
            continue;
        };

        visits.push((question_id, event.timestamp.into()));
    }

    let questions = || {
        attempt
            .question_sets
            .iter()
            .flat_map(|qs| qs.questions.iter())
    };

    let mut times = vec![];

    for question in questions() {
        let Some(submission_time) = question.submission_time else {
            continue;
        };

        let visit_start = visits
            .iter()
            .filter(|(id, timestamp)| *id == question.id && *timestamp <= submission_time)
            .map(|(_, timestamp)| *timestamp)
            .max();

        let start = visit_start
            .or_else(|| {
                // Nearest earlier submission of any other question: answering
                // that question is the latest point this one could have been
                // started from.
                questions()
                    .filter(|q| q.id != question.id)
                    .filter_map(|q| q.submission_time)
                    .filter(|t| *t < submission_time)
                    .max()
            })
            .unwrap_or(attempt.start_time);

        times.push(submission_time.saturating_duration_since(start));
    }

    times
}

pub fn get_total_time(attempt: &Attempt) -> Duration {
    let last_submission_time = get_last_submission_time(&attempt);
    last_submission_time.saturating_duration_since(attempt.start_time)
}

/// TODO: Test for fault-tolerance
///       - What happens in cases where events are not recorded well
///       - e.g. question navigation/answer during blur period
pub fn get_blur_periods(events: &Vec<Event>) -> Vec<QuestionBlurPeriods> {
    // Blur/Focus are window-level (tab focus lost/regained), not per-question:
    // "meta.question" only annotates which question happened to be showing at
    // that moment. Bucketing pairs by question (as a prior version of this
    // function did) treats navigating between questions as if each question
    // tracked its own independent focus state, so a blur left pending on
    // question A while the user moves on to answer B..Z is only "closed" once
    // the user happens to revisit A - counting that entire in-between span
    // (during which the tab was actually focused, just on other questions) as
    // blur time. Those spans overlap across questions, so summed blur time can
    // exceed the attempt's actual wall-clock duration. Pair chronologically
    // across the whole event stream instead: at any instant the tab is either
    // focused or blurred, never both.
    let mut blur_periods: Vec<QuestionBlurPeriods> = vec![];
    let mut pending_blur = None;
    for event in events {
        if !matches!(event.kind, EventKind::Blur | EventKind::Focus) {
            continue;
        }
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
                // A second blur before a focus closes the first: keep the
                // earlier start, treat it as one continuous blur period.
                pending_blur.get_or_insert((event.timestamp, question_id));
            }
            EventKind::Focus => {
                // A focus with no pending blur has no matching pair: ignore it.
                if let Some((blur_start, blur_question_id)) = pending_blur.take() {
                    let timestamp = event.timestamp;
                    // let blur_time = (timestamp - blur_start).as_seconds_f64();
                    let period = Period {
                        start: blur_start.into(),
                        end: timestamp.into(),
                    };

                    match blur_periods
                        .iter_mut()
                        .find(|q| q.question_id == blur_question_id)
                    {
                        Some(q) => q.periods.push(period),
                        None => blur_periods.push(QuestionBlurPeriods {
                            question_id: blur_question_id,
                            periods: vec![period],
                        }),
                    }
                }
            }
            _ => unreachable!("filtered to only Blur/Focus events above"),
        }
    }
    // A trailing pending blur with no closing focus has no matching pair:
    // its end is unknown, so it is dropped rather than guessed at.
    blur_periods
}

pub fn get_total_blur_time(events: &Vec<Event>) -> f64 {
    let mut total_blur_time = 0.0;
    let mut prev_blur = None;
    let mut events = events.clone();
    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    for event in &events {
        match event.kind {
            EventKind::Blur => {
                if prev_blur.is_none() {
                    prev_blur = Some(event.timestamp);
                }
            }
            EventKind::Focus => {
                // A focus with no pending blur has no matching pair: ignore it.
                if let Some(prev) = prev_blur {
                    let timestamp = event.timestamp;
                    let blur_time = (timestamp - prev).as_seconds_f64();
                    total_blur_time += blur_time;
                    prev_blur = None;
                }
            }
            _ => {}
        }
    }

    total_blur_time
}

pub fn get_total_blur_time_before_last_answer(attempt: &Attempt, events: &Vec<Event>) -> f64 {
    let mut total_blur_time_before_last_answer = 0.0;

    let last_submission_time = get_last_submission_time(&attempt);

    let mut prev_blur = None;
    let mut events = events.clone();
    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    for event in &events {
        match event.kind {
            EventKind::Blur => {
                if prev_blur.is_none() {
                    prev_blur = Some(event.timestamp);
                }
            }
            EventKind::Focus => {
                // A focus with no pending blur has no matching pair: ignore it.
                if let Some(prev) = prev_blur {
                    let timestamp = event.timestamp;

                    if timestamp.timestamp_millis() < last_submission_time.timestamp_millis() {
                        let blur_time = (timestamp - prev).as_seconds_f64();
                        total_blur_time_before_last_answer += blur_time;
                    }
                    prev_blur = None;
                }
            }
            _ => {}
        }
    }

    total_blur_time_before_last_answer
}

pub fn get_last_submission_time(attempt: &Attempt) -> DateTime {
    attempt
        .question_sets
        .iter()
        .flat_map(|qs| qs.questions.iter().flat_map(|q| q.submission_time))
        .max()
        .expect("at least one question to have been answered")
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

            let score = match get_moderation_score(&attempt, &events)
                .map_err(|e| format!("{}: {e}", attempt.id))
            {
                Ok(s) => s,
                Err(e) => {
                    println!("{}", e);
                    panic!("score calc error");
                }
            };

            if score < min {
                min = score;
            }
            if score > max {
                max = score;
            }

            scores.push(format!("{:.3}", score));
        }

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

/// Synthetic, fixture-independent unit tests for the pure attempt logic.
///
/// These build minimal `Attempt`/`Event` values by hand so each scoring
/// component and helper is exercised in isolation with exact expected values -
/// unlike the fixture aggregate tests above, which only bound the output range.
///
/// `get_time_between_submissions` and `get_attempt_stats` are `todo!()` stubs
/// and so are intentionally not covered here.
#[cfg(test)]
mod logic {
    use bson::{DateTime, oid::ObjectId};
    use prisma::{
        ExamEnvironmentAnswer, ExamEnvironmentConfig, ExamEnvironmentExam,
        ExamEnvironmentExamAttempt, ExamEnvironmentGeneratedExam,
        ExamEnvironmentMultipleChoiceQuestion, ExamEnvironmentMultipleChoiceQuestionAttempt,
        ExamEnvironmentQuestionSet, ExamEnvironmentQuestionSetAttempt,
        supabase::{Event, EventKind},
    };
    use std::time::Duration;

    use crate::attempt::{
        Attempt, AttemptQuestionSet, AttemptQuestionSetQuestion, Median, construct_attempt,
        get_blur_periods, get_last_submission_time, get_moderation_score, get_time_per_question,
        get_total_blur_time, get_total_blur_time_before_last_answer, get_total_time,
    };
    use crate::error::Error;

    /// Fixed epoch offset - keeps timestamps well clear of 0 so nothing
    /// accidentally saturates against the Unix epoch.
    const T0: i64 = 1_700_000_000_000;

    /// Deterministic distinct `ObjectId` from a small index.
    fn oid(n: u8) -> ObjectId {
        let mut bytes = [0u8; 12];
        bytes[11] = n;
        ObjectId::from_bytes(bytes)
    }

    /// `bson::DateTime` at `T0 + ms`.
    fn bdt(ms: i64) -> DateTime {
        DateTime::from_millis(T0 + ms)
    }

    /// Build an event at `T0 + ms`. `question` populates `meta.question`; `None`
    /// leaves `meta` empty (exercises the missing-meta skip path).
    fn ev(kind: EventKind, question: Option<ObjectId>, ms: i64) -> Event {
        Event {
            id: String::from("evt"),
            // bson -> chrono keeps whole-millisecond precision, so it round-trips
            // exactly against the bson submission times used below.
            timestamp: DateTime::from_millis(T0 + ms).to_chrono(),
            kind,
            meta: match question {
                Some(qid) => serde_json::json!({ "question": qid.to_hex() }),
                None => serde_json::json!({}),
            },
            attempt_id: oid(0),
        }
    }

    /// Attempt question with only id + optional submission time set (all other
    /// fields empty) - enough for the timing/blur/moderation logic.
    fn q(id: ObjectId, submission_ms: Option<i64>) -> AttemptQuestionSetQuestion {
        AttemptQuestionSetQuestion {
            id,
            text: String::new(),
            tags: vec![],
            deprecated: false,
            audio: None,
            answers: vec![],
            selected: vec![],
            generated: vec![],
            submission_time: submission_ms.map(bdt),
        }
    }

    /// Single-question-set attempt with a chosen start, total time and passing
    /// percent.
    fn attempt_with(
        start_ms: i64,
        total_time_in_s: i64,
        passing_percent: f64,
        questions: Vec<AttemptQuestionSetQuestion>,
    ) -> Attempt {
        Attempt {
            id: oid(250),
            exam_id: oid(251),
            user_id: oid(252),
            prerequisites: vec![],
            deprecated: false,
            question_sets: vec![AttemptQuestionSet {
                id: oid(253),
                _type: Default::default(),
                context: None,
                questions,
            }],
            config: ExamEnvironmentConfig {
                total_time_in_s,
                passing_percent,
                ..Default::default()
            },
            start_time: bdt(start_ms),
        }
    }

    // ---- Median -----------------------------------------------------------

    #[test]
    fn median_empty_is_none() {
        let v: Vec<Duration> = vec![];
        assert_eq!(v.median(), None);
    }

    #[test]
    fn median_odd_len_returns_middle_and_sorts() {
        let v = vec![
            Duration::from_secs(3),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ];
        assert_eq!(v.median(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn median_even_len_averages_middle_pair() {
        let v = vec![Duration::from_secs(2), Duration::from_secs(4)];
        assert_eq!(v.median(), Some(Duration::from_secs(3)));
    }

    // ---- get_last_submission_time / get_total_time ------------------------

    #[test]
    fn last_submission_time_is_max() {
        let a = attempt_with(
            0,
            100,
            80.0,
            vec![
                q(oid(1), Some(5_000)),
                q(oid(2), Some(9_000)),
                q(oid(3), None),
                q(oid(4), Some(7_000)),
            ],
        );
        assert_eq!(get_last_submission_time(&a), bdt(9_000));
    }

    #[test]
    #[should_panic(expected = "at least one question")]
    fn last_submission_time_panics_when_none_answered() {
        let a = attempt_with(0, 100, 80.0, vec![q(oid(1), None)]);
        let _ = get_last_submission_time(&a);
    }

    #[test]
    fn total_time_is_last_submission_minus_start() {
        let a = attempt_with(
            0,
            100,
            80.0,
            vec![q(oid(1), Some(3_000)), q(oid(2), Some(8_000))],
        );
        assert_eq!(get_total_time(&a), Duration::from_secs(8));
    }

    #[test]
    fn total_time_saturates_to_zero_when_submission_before_start() {
        let a = attempt_with(10_000, 100, 80.0, vec![q(oid(1), Some(3_000))]);
        assert_eq!(get_total_time(&a), Duration::ZERO);
    }

    // ---- get_time_per_question --------------------------------------------

    #[test]
    fn time_per_question_uses_latest_visit_at_or_before_submission() {
        let q1 = oid(1);
        let a = attempt_with(0, 100, 80.0, vec![q(q1, Some(10_000))]);
        let events = vec![
            ev(EventKind::QuestionVisit, Some(q1), 2_000),
            ev(EventKind::QuestionVisit, Some(q1), 8_000),
            // After submission -> excluded by the `<= submission_time` filter.
            ev(EventKind::QuestionVisit, Some(q1), 12_000),
        ];
        // 10_000 - 8_000 (latest visit at/before submission)
        assert_eq!(
            get_time_per_question(&a, &events),
            vec![Duration::from_secs(2)]
        );
    }

    #[test]
    fn time_per_question_falls_back_to_nearest_earlier_submission_then_start() {
        let q1 = oid(1);
        let q2 = oid(2);
        let a = attempt_with(0, 100, 80.0, vec![q(q1, Some(5_000)), q(q2, Some(12_000))]);
        // No visits:
        //   q1 -> no earlier other submission -> start_time(0)      => 5s
        //   q2 -> nearest earlier other submission is q1 @5s        => 7s
        assert_eq!(
            get_time_per_question(&a, &vec![]),
            vec![Duration::from_secs(5), Duration::from_secs(7)]
        );
    }

    #[test]
    fn time_per_question_ignores_other_question_visits_and_unanswered() {
        let q1 = oid(1);
        let q2 = oid(2);
        let a = attempt_with(0, 100, 80.0, vec![q(q1, Some(6_000)), q(q2, None)]);
        // Visit belongs to q2, which is unanswered (omitted); q1 has no own
        // visit and no earlier other submission -> start_time(0) => 6s.
        let events = vec![ev(EventKind::QuestionVisit, Some(q2), 3_000)];
        assert_eq!(
            get_time_per_question(&a, &events),
            vec![Duration::from_secs(6)]
        );
    }

    // ---- get_blur_periods -------------------------------------------------

    #[test]
    fn blur_periods_pairs_blur_with_next_focus() {
        let q1 = oid(1);
        let events = vec![
            ev(EventKind::Blur, Some(q1), 1_000),
            ev(EventKind::Focus, Some(q1), 3_000),
        ];
        let bps = get_blur_periods(&events);
        assert_eq!(bps.len(), 1);
        assert_eq!(bps[0].question_id, q1);
        assert_eq!(bps[0].periods.len(), 1);
        assert_eq!(bps[0].periods[0].start.timestamp_millis(), T0 + 1_000);
        assert_eq!(bps[0].periods[0].end.timestamp_millis(), T0 + 3_000);
    }

    #[test]
    fn blur_periods_second_blur_before_focus_keeps_first_start_and_question() {
        let q1 = oid(1);
        let q2 = oid(2);
        let events = vec![
            ev(EventKind::Blur, Some(q1), 1_000),
            // Second blur before a focus is window-level, not per-question: it is
            // folded into the still-open period, keeping the first start + question.
            ev(EventKind::Blur, Some(q2), 1_500),
            ev(EventKind::Focus, Some(q2), 3_000),
        ];
        let bps = get_blur_periods(&events);
        assert_eq!(bps.len(), 1);
        assert_eq!(bps[0].question_id, q1);
        assert_eq!(bps[0].periods.len(), 1);
        assert_eq!(bps[0].periods[0].start.timestamp_millis(), T0 + 1_000);
        assert_eq!(bps[0].periods[0].end.timestamp_millis(), T0 + 3_000);
    }

    #[test]
    fn blur_periods_ignores_unpaired_focus_and_trailing_blur() {
        let q1 = oid(1);
        let events = vec![
            ev(EventKind::Focus, Some(q1), 1_000), // no pending blur -> ignored
            ev(EventKind::Blur, Some(q1), 2_000),  // never closed -> dropped
        ];
        assert!(get_blur_periods(&events).is_empty());
    }

    #[test]
    fn blur_periods_aggregates_same_question_and_skips_irrelevant_events() {
        let q1 = oid(1);
        let events = vec![
            ev(EventKind::QuestionVisit, Some(q1), 500), // not blur/focus -> skipped
            ev(EventKind::Blur, Some(q1), 1_000),
            ev(EventKind::Focus, Some(q1), 2_000),
            ev(EventKind::Blur, None, 2_500), // no question meta -> skipped
            ev(EventKind::Blur, Some(q1), 3_000),
            ev(EventKind::Focus, Some(q1), 4_000),
        ];
        let bps = get_blur_periods(&events);
        assert_eq!(bps.len(), 1);
        assert_eq!(bps[0].periods.len(), 2);
    }

    // ---- get_total_blur_time ----------------------------------------------

    #[test]
    fn total_blur_time_sorts_and_sums_focus_minus_blur() {
        let q1 = oid(1);
        // Deliberately unsorted: the function sorts internally.
        let events = vec![
            ev(EventKind::Focus, Some(q1), 3_000),
            ev(EventKind::Blur, Some(q1), 1_000),
        ];
        assert_eq!(get_total_blur_time(&events), 2.0);
    }

    #[test]
    fn total_blur_time_double_blur_uses_first_start_and_drops_trailing() {
        let q1 = oid(1);
        let events = vec![
            ev(EventKind::Blur, Some(q1), 1_000),
            ev(EventKind::Blur, Some(q1), 1_500), // ignored while a blur is pending
            ev(EventKind::Focus, Some(q1), 3_000),
            ev(EventKind::Blur, Some(q1), 5_000), // trailing, no focus -> dropped
        ];
        assert_eq!(get_total_blur_time(&events), 2.0);
    }

    #[test]
    fn total_blur_time_sums_multiple_periods() {
        let q1 = oid(1);
        let events = vec![
            ev(EventKind::Blur, Some(q1), 1_000),
            ev(EventKind::Focus, Some(q1), 2_000),
            ev(EventKind::Blur, Some(q1), 5_000),
            ev(EventKind::Focus, Some(q1), 5_500),
        ];
        assert_eq!(get_total_blur_time(&events), 1.5);
    }

    // ---- get_total_blur_time_before_last_answer ---------------------------

    #[test]
    fn total_blur_time_before_last_answer_excludes_after_last_submission() {
        let q1 = oid(1);
        let a = attempt_with(0, 1000, 80.0, vec![q(q1, Some(10_000))]);
        let events = vec![
            ev(EventKind::Blur, Some(q1), 1_000),
            ev(EventKind::Focus, Some(q1), 2_000), // end 2s  < 10s -> counted
            ev(EventKind::Blur, Some(q1), 11_000),
            ev(EventKind::Focus, Some(q1), 12_000), // end 12s !< 10s -> excluded
        ];
        assert_eq!(get_total_blur_time_before_last_answer(&a, &events), 1.0);
    }

    #[test]
    fn total_blur_time_before_last_answer_excludes_focus_at_exactly_last_submission() {
        let q1 = oid(1);
        let a = attempt_with(0, 1000, 80.0, vec![q(q1, Some(10_000))]);
        let events = vec![
            ev(EventKind::Blur, Some(q1), 9_000),
            // end == last submission; comparison is strict `<` -> excluded.
            ev(EventKind::Focus, Some(q1), 10_000),
        ];
        assert_eq!(get_total_blur_time_before_last_answer(&a, &events), 0.0);
    }

    // ---- get_moderation_score (per-component isolation) -------------------

    #[test]
    fn moderation_score_zero_when_nothing_answered() {
        let a = attempt_with(0, 100, 80.0, vec![q(oid(1), None), q(oid(2), None)]);
        assert_eq!(get_moderation_score(&a, &vec![]).unwrap(), 0.0);
    }

    #[test]
    fn moderation_score_errors_when_time_taken_exceeds_total_time() {
        // total_time = 10s, but last submission is at 20s.
        let a = attempt_with(0, 10, 80.0, vec![q(oid(1), Some(20_000))]);
        let err = get_moderation_score(&a, &vec![]).unwrap_err();
        assert!(matches!(err, Error::ModerationScore(_)));
    }

    #[test]
    fn moderation_score_clean_attempt_is_zero() {
        // 4 questions 6s apart; generous total time; no blur.
        //   fast-completion: 24/100 = 0.24  (>= 0.1) -> 0
        //   median per question (fallback) = 6s (>= 5) -> 0
        //   blur -> 0
        let a = attempt_with(
            0,
            100,
            80.0,
            vec![
                q(oid(1), Some(6_000)),
                q(oid(2), Some(12_000)),
                q(oid(3), Some(18_000)),
                q(oid(4), Some(24_000)),
            ],
        );
        assert_eq!(get_moderation_score(&a, &vec![]).unwrap(), 0.0);
    }

    #[test]
    fn moderation_score_fast_completion_adds_expected() {
        // Huge total time -> percent << 0.1 (fast-completion fires), while
        // per-question times stay >= 5s so the median component contributes 0.
        let a = attempt_with(
            0,
            1000,
            80.0,
            vec![q(oid(1), Some(5_000)), q(oid(2), Some(10_000))],
        );
        // percent = 10/1000 = 0.01 -> |0.01 - 0.1|/0.1 * 0.25 = 0.9 * 0.25 = 0.225
        let score = get_moderation_score(&a, &vec![]).unwrap();
        assert!((score - 0.225).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn moderation_score_low_median_time_adds_expected() {
        // Small total time keeps percent >= 0.1 (no fast-completion score); tiny
        // per-question times drive the median component alone.
        let a = attempt_with(
            0,
            10,
            80.0,
            vec![
                q(oid(1), Some(1_000)),
                q(oid(2), Some(2_000)),
                q(oid(3), Some(3_000)),
            ],
        );
        // percent = 3/10 = 0.3 (no fast score); per-question times all 1s;
        // median score = |1 - 5|/5 * 0.25 = 0.8 * 0.25 = 0.2
        let score = get_moderation_score(&a, &vec![]).unwrap();
        assert!((score - 0.2).abs() < 1e-9, "score={score}");
    }

    #[test]
    fn moderation_score_blur_before_last_answer_adds_expected() {
        let q1 = oid(1);
        // 10 questions 6s apart -> last submission 60s.
        let questions = (1..=10u8)
            .map(|i| q(oid(i), Some(i as i64 * 6_000)))
            .collect::<Vec<_>>();
        let a = attempt_with(0, 500, 80.0, questions);
        // percent = 60/500 = 0.12 (no fast score); median = 6s (no median score).
        // One question blurred before last submission.
        let events = vec![
            ev(EventKind::Blur, Some(q1), 1_000),
            ev(EventKind::Focus, Some(q1), 2_000),
        ];
        // blurred_percent = 1/10 = 0.1; passing_percent = 80
        // weight = min(0.1*100/(100-80), 1) * 0.25 = min(0.5, 1) * 0.25 = 0.125
        let score = get_moderation_score(&a, &events).unwrap();
        assert!((score - 0.125).abs() < 1e-9, "score={score}");
    }

    // ---- construct_attempt ------------------------------------------------

    fn answer(id: ObjectId, is_correct: bool) -> ExamEnvironmentAnswer {
        ExamEnvironmentAnswer {
            id,
            is_correct,
            text: String::new(),
        }
    }

    #[test]
    fn construct_attempt_merges_selected_generated_and_submission() {
        let (qs1, q1, q2) = (oid(10), oid(11), oid(12));
        let (a1, a2, a3, a4) = (oid(13), oid(14), oid(15), oid(16));
        let (exam_id, user_id, attempt_id, gen_id, prereq) =
            (oid(20), oid(21), oid(22), oid(23), oid(24));

        let exam = ExamEnvironmentExam {
            question_sets: vec![ExamEnvironmentQuestionSet {
                id: qs1,
                _type: Default::default(),
                context: Some(String::from("ctx")),
                questions: vec![
                    ExamEnvironmentMultipleChoiceQuestion {
                        id: q1,
                        text: String::from("q1"),
                        tags: vec![String::from("tag")],
                        audio: None,
                        answers: vec![answer(a1, true), answer(a2, false)],
                        deprecated: false,
                    },
                    ExamEnvironmentMultipleChoiceQuestion {
                        id: q2,
                        text: String::from("q2"),
                        tags: vec![],
                        audio: None,
                        answers: vec![answer(a3, true), answer(a4, false)],
                        deprecated: false,
                    },
                ],
            }],
            prerequisites: vec![prereq],
            deprecated: true,
            ..Default::default()
        };

        // `ExamEnvironmentGeneratedExam` has no Default impl; build it from the
        // extended-JSON shape the fixtures use.
        let generation: ExamEnvironmentGeneratedExam = serde_json::from_value(serde_json::json!({
            "_id": { "$oid": gen_id.to_hex() },
            "examId": { "$oid": exam_id.to_hex() },
            "questionSets": [{
                "id": { "$oid": qs1.to_hex() },
                "questions": [
                    { "id": { "$oid": q1.to_hex() }, "answers": [ { "$oid": a1.to_hex() }, { "$oid": a2.to_hex() } ] },
                    { "id": { "$oid": q2.to_hex() }, "answers": [ { "$oid": a3.to_hex() } ] },
                ]
            }],
            "deprecated": false,
            "version": 1,
        }))
        .unwrap();

        // Attempt answers only q1.
        let exam_attempt = ExamEnvironmentExamAttempt {
            id: attempt_id,
            exam_id,
            user_id,
            generated_exam_id: gen_id,
            start_time: bdt(1_000),
            question_sets: vec![ExamEnvironmentQuestionSetAttempt {
                id: qs1,
                questions: vec![ExamEnvironmentMultipleChoiceQuestionAttempt {
                    id: q1,
                    answers: vec![a1],
                    submission_time: bdt(5_000),
                }],
            }],
            ..Default::default()
        };

        let attempt = construct_attempt(&exam, &generation, &exam_attempt);

        // Top-level fields sourced from the right inputs.
        assert_eq!(attempt.id, attempt_id);
        assert_eq!(attempt.exam_id, exam_id);
        assert_eq!(attempt.user_id, user_id);
        assert_eq!(attempt.prerequisites, vec![prereq]);
        assert!(attempt.deprecated);
        assert_eq!(attempt.start_time, bdt(1_000));

        assert_eq!(attempt.question_sets.len(), 1);
        let out_qs = &attempt.question_sets[0];
        assert_eq!(out_qs.id, qs1);
        assert_eq!(out_qs.context.as_deref(), Some("ctx"));
        assert_eq!(out_qs.questions.len(), 2);

        // q1: answered -> selected from attempt, generated from generation,
        // submission set, and all exam answers retained.
        let oq1 = &out_qs.questions[0];
        assert_eq!(oq1.id, q1);
        assert_eq!(oq1.selected, vec![a1]);
        assert_eq!(oq1.generated, vec![a1, a2]);
        assert_eq!(oq1.submission_time, Some(bdt(5_000)));
        assert_eq!(oq1.answers.len(), 2);

        // q2: unanswered -> no selected, no submission; generated still filled
        // from the generation.
        let oq2 = &out_qs.questions[1];
        assert_eq!(oq2.id, q2);
        assert!(oq2.selected.is_empty());
        assert_eq!(oq2.generated, vec![a3]);
        assert_eq!(oq2.submission_time, None);
    }

    #[test]
    fn construct_attempt_question_set_absent_from_generation_and_attempt_is_empty() {
        let (qs2, q3, a5) = (oid(30), oid(31), oid(32));
        let (exam_id, gen_id) = (oid(40), oid(41));

        // Exam has a question set that neither the generation nor the attempt
        // references.
        let exam = ExamEnvironmentExam {
            question_sets: vec![ExamEnvironmentQuestionSet {
                id: qs2,
                _type: Default::default(),
                context: None,
                questions: vec![ExamEnvironmentMultipleChoiceQuestion {
                    id: q3,
                    text: String::from("q3"),
                    tags: vec![],
                    audio: None,
                    answers: vec![answer(a5, true)],
                    deprecated: false,
                }],
            }],
            ..Default::default()
        };

        let generation: ExamEnvironmentGeneratedExam = serde_json::from_value(serde_json::json!({
            "_id": { "$oid": gen_id.to_hex() },
            "examId": { "$oid": exam_id.to_hex() },
            "questionSets": [],
            "deprecated": false,
            "version": 1,
        }))
        .unwrap();

        let exam_attempt = ExamEnvironmentExamAttempt {
            exam_id,
            generated_exam_id: gen_id,
            question_sets: vec![],
            ..Default::default()
        };

        let attempt = construct_attempt(&exam, &generation, &exam_attempt);

        assert_eq!(attempt.question_sets.len(), 1);
        let oq = &attempt.question_sets[0].questions[0];
        assert_eq!(oq.id, q3);
        assert!(oq.selected.is_empty());
        assert!(oq.generated.is_empty());
        assert_eq!(oq.submission_time, None);
        // Exam answers are still carried over verbatim.
        assert_eq!(oq.answers.len(), 1);
    }
}
