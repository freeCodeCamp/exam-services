use mongodb::bson::oid::ObjectId;
use prisma::{
    self,
    supabase::{Event, EventKind},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    pub start_time: mongodb::bson::DateTime,
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
    pub submission_time: Option<mongodb::bson::DateTime>,
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

pub fn get_attempt_stats(_attempt: Attempt) -> AttemptStats {
    todo!()
}

/// Calculates a 0.0 -> 1.0 score.
///
/// - A score of 0.0 means the attempt definitely does not need moderation.
/// - A score of 1.0 means the attempt definitely does need moderation.
pub fn get_moderation_score(attempt: &Attempt, events: &Vec<Event>) -> f64 {
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
            return moderation_score;
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

    assert!(
        total_time_taken <= total_time,
        "{total_time_taken} <= {total_time}"
    );
    assert!(
        total_blur_time <= total_time,
        "{total_blur_time} <= {total_time}"
    );
    assert!(
        total_blur_time_before_last_answer <= total_blur_time,
        "{total_blur_time_before_last_answer} <= {total_blur_time}"
    );

    let time_weight = ((total_time - total_time_taken) / total_time) * weight;
    assert!(time_weight <= weight, "{time_weight} <= {}", weight);
    moderation_score += time_weight;

    // Blur time after last submission is worth 1/3 as much as before last submission
    // Seeing as both total_blur_time_* vars include the time before, it is counted 'twice'
    let blur_weight = (total_blur_time / total_time) * weight;
    assert!(blur_weight <= weight, "{blur_weight} <= {weight}");
    moderation_score += blur_weight;

    assert!(
        total_blur_time_before_last_answer <= total_time_taken,
        "{total_blur_time_before_last_answer} <= {total_time_taken}"
    );
    let blur_before_weight = (total_blur_time_before_last_answer / total_time_taken) * weight * 2.0;
    assert!(
        blur_before_weight <= weight * 2.0,
        "{blur_before_weight} <= {}",
        weight * 2.0
    );
    moderation_score += blur_before_weight;

    if moderation_score > 1.0 {
        tracing::error!(
            attempt = %attempt.id,
            moderation_score,
            "moderation score should never be > 1.0"
        );
    }

    moderation_score
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

            let score = get_moderation_score(&attempt, &events);

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
}
