use wasm_bindgen::prelude::*;

use crate::parse;

/// Validates a generated exam for basic properties:
/// 1. No duplicate question set, question, or answer ids
///
/// @param {ExamEnvironmentGeneratedExam} generation
/// @returns {string | undefined} the validation error, or `undefined` when valid
/// @throws {Error} if the argument does not deserialize
#[wasm_bindgen]
pub fn validate_generation(generation: JsValue) -> Result<Option<String>, JsError> {
    let generation: prisma::ExamEnvironmentGeneratedExam = parse("generation", generation)?;

    match exam_utils::generation::validate_generation(&generation) {
        Ok(()) => Ok(None),
        Err(e) => Ok(Some(e.to_string())),
    }
}
