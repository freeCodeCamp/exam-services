use wasm_bindgen::prelude::*;

use crate::{parse, serialize};

/// Combines an exam, one of its generated exams, and a user attempt into a
/// single, self-contained `Attempt` object:
/// - Questions are taken from the exam
/// - `generated` answers come from the generated exam
/// - `selected` answers and `submissionTime` come from the attempt
///
/// @param {ExamEnvironmentExam} exam
/// @param {ExamEnvironmentGeneratedExam} generation
/// @param {ExamEnvironmentExamAttempt} attempt
/// @returns {Attempt}
/// @throws {Error} if an argument does not deserialize
#[wasm_bindgen]
pub fn construct_attempt(
    exam: JsValue,
    generation: JsValue,
    attempt: JsValue,
) -> Result<JsValue, JsError> {
    let exam: prisma::ExamEnvironmentExam = parse("exam", exam)?;
    let generation: prisma::ExamEnvironmentGeneratedExam = parse("generation", generation)?;
    let attempt: prisma::ExamEnvironmentExamAttempt = parse("attempt", attempt)?;

    let res = exam_utils::attempt::construct_attempt(&exam, &generation, &attempt);

    serialize(&res)
}

/// Calculates a `0.0 -> 1.0` moderation score for an attempt.
///
/// - `0.0`: the attempt definitely does **not** need moderation
/// - `1.0`: the attempt definitely does need moderation
///
/// @param {Attempt} attempt - as returned by `construct_attempt`
/// @param {Event[]} events - proctoring events recorded during the attempt
/// @throws {Error} if an argument does not deserialize, or if the event
/// timings are inconsistent with the attempt
#[wasm_bindgen]
pub fn get_moderation_score(attempt: JsValue, events: JsValue) -> Result<f64, JsError> {
    let attempt: exam_utils::attempt::Attempt = parse("attempt", attempt)?;
    let events: Vec<prisma::supabase::Event> = parse("events", events)?;

    let score = exam_utils::attempt::get_moderation_score(&attempt, &events)
        .map_err(|e| JsError::new(&e.to_string()))?;

    Ok(score)
}
