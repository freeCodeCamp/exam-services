use bson::oid::ObjectId;
use wasm_bindgen::prelude::*;

use crate::{parse, serialize};

/// Calculates the attempt score, and compares `score >= config.passingPercent`.
///
/// Returns `false` if the score cannot be calculated (e.g. the attempt
/// references questions that do not exist in the exam or generation).
///
/// @param {ExamEnvironmentExam} exam
/// @param {ExamEnvironmentGeneratedExam} generation
/// @param {ExamEnvironmentExamAttempt} attempt
/// @throws {Error} if an argument does not deserialize
#[wasm_bindgen]
pub fn check_attempt_pass(
    exam: JsValue,
    generation: JsValue,
    attempt: JsValue,
) -> Result<bool, JsError> {
    let exam: prisma::ExamEnvironmentExam = parse("exam", exam)?;
    let generation: prisma::ExamEnvironmentGeneratedExam = parse("generation", generation)?;
    let attempt: prisma::ExamEnvironmentExamAttempt = parse("attempt", attempt)?;

    Ok(exam_utils::misc::check_attempt_pass(
        &exam,
        &generation,
        &attempt,
    ))
}

/// Calculates the attempt score as a `0.0 -> 100.0` percentage of correctly
/// answered questions.
///
/// @param {ExamEnvironmentExam} exam
/// @param {ExamEnvironmentGeneratedExam} generation
/// @param {ExamEnvironmentExamAttempt} attempt
/// @throws {Error} if an argument does not deserialize, or if the attempt
/// references questions that do not exist in the exam or generation
#[wasm_bindgen]
pub fn calculate_score(
    exam: JsValue,
    generation: JsValue,
    attempt: JsValue,
) -> Result<f64, JsError> {
    let exam: prisma::ExamEnvironmentExam = parse("exam", exam)?;
    let generation: prisma::ExamEnvironmentGeneratedExam = parse("generation", generation)?;
    let attempt: prisma::ExamEnvironmentExamAttempt = parse("attempt", attempt)?;

    exam_utils::misc::calculate_score(&exam, &generation, &attempt)
        .map_err(|e| JsError::new(&e))
}

/// Compares the answers of a single question: `true` when the attempt selected
/// exactly the correct answers shown in the generation.
///
/// @param {ExamEnvironmentAnswer[]} exam_answers - all answers, with `isCorrect`
/// @param {ObjectId[]} generation_answers - answer ids shown to the user
/// @param {ObjectId[]} attempt_answers - answer ids selected by the user
/// @throws {Error} if an argument does not deserialize
#[wasm_bindgen]
pub fn compare_answers(
    exam_answers: JsValue,
    generation_answers: JsValue,
    attempt_answers: JsValue,
) -> Result<bool, JsError> {
    let exam_answers: Vec<prisma::ExamEnvironmentAnswer> = parse("exam_answers", exam_answers)?;
    let generation_answers: Vec<ObjectId> = parse("generation_answers", generation_answers)?;
    let attempt_answers: Vec<ObjectId> = parse("attempt_answers", attempt_answers)?;

    Ok(exam_utils::misc::compare_answers(
        &exam_answers,
        &generation_answers,
        &attempt_answers,
    ))
}

/// Validates an exam config:
/// - `config.name` is not empty
/// - `config.passingPercent` is between 0 and 100
/// - `config.tags` and `config.questionSets` are solvable
/// - questions have text, and at least one correct answer with text
///
/// @param {ExamEnvironmentExam} exam
/// @returns {string | undefined} the validation error, or `undefined` when valid
/// @throws {Error} if the argument does not deserialize
#[wasm_bindgen]
pub fn validate_config(exam: JsValue) -> Result<Option<String>, JsError> {
    let exam: prisma::ExamEnvironmentExam = parse("exam", exam)?;

    match exam_utils::misc::validate_config(&exam) {
        Ok(()) => Ok(None),
        Err(e) => Ok(Some(e)),
    }
}

/// Generates a randomized exam for a user, based on the exam configuration.
///
/// @param {ExamEnvironmentExam} exam - only `_id`/`id`, `questionSets`, and
/// `config` are used
/// @returns {ExamEnvironmentGeneratedExam}
/// @throws {Error} if the argument does not deserialize, or if the exam
/// config cannot be satisfied
#[wasm_bindgen]
pub fn generate_exam(exam: JsValue) -> Result<JsValue, JsError> {
    let exam: exam_utils::misc::ExamInput = parse("exam", exam)?;

    let generated = exam_utils::misc::generate_exam(exam).map_err(|e| JsError::new(&e.to_string()))?;

    serialize(&generated)
}
