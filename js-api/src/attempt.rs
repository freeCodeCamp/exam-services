use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

#[wasm_bindgen]
pub fn construct_attempt(exam: JsValue, generation: JsValue, attempt: JsValue) -> JsValue {
    let exam: prisma::ExamEnvironmentExam = from_value(exam).unwrap();
    let generation: prisma::ExamEnvironmentGeneratedExam = from_value(generation).unwrap();
    let attempt: prisma::ExamEnvironmentExamAttempt = from_value(attempt).unwrap();
    let res = exam_utils::attempt::construct_attempt(&exam, &generation, &attempt);

    to_value(&res).unwrap()
}

#[wasm_bindgen]
pub fn get_moderation_score(attempt: JsValue, events: JsValue) -> JsValue {
    let attempt: exam_utils::attempt::Attempt = from_value(attempt).unwrap();
    let events: Vec<prisma::supabase::Event> = from_value(events).unwrap();

    let res = exam_utils::attempt::get_moderation_score(&attempt, &events);

    to_value(&res).unwrap()
}
