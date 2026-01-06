use mongodb::bson::oid::ObjectId;
use prisma;
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
