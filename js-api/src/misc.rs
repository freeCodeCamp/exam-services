#[wasm_bindgen]
pub fn check_attempt_pass(exam: JsValue, generation: JsValue, attempt: JsValue) -> JsValue {
    let exam: prisma::ExamEnvironmentExam = from_value(exam).unwrap();
    let generation: prisma::ExamEnvironmentGeneratedExam = from_value(generation).unwrap();
    let attempt: prisma::ExamEnvironmentExamAttempt = from_value(attempt).unwrap();

    let res = exam_utils::misc::check_attempt_pass(&exam, &generation, &attempt);

    to_value(res).unwrap()
}

#[wasm_bindgen]
pub fn calculate_score(exam: JsValue, generation: JsValue, attempt: JsValue) -> JsValue {
    let exam: prisma::ExamEnvironmentExam = from_value(exam).unwrap();
    let generation: prisma::ExamEnvironmentGeneratedExam = from_value(generation).unwrap();
    let attempt: prisma::ExamEnvironmentExamAttempt = from_value(attempt).unwrap();

    let res = exam_utils::misc::calculate_score(&exam, &generation, &attempt);

    to_value(res).unwrap()
}

#[wasm_bindgen]
pub fn compare_answers(exam_answers: JsValue, generation_answers: JsValue, attempt_answers: JsValue) -> JsValue {
    let exam_answers: Vec<prisma::ExamEnvironmentAnswer> = from_value(exam_answers).unwrap();
    let generation_answers: Vec<ObjectId> = from_value(generation_answers: JsValue).unwrap();
    let attempt_answers: Vec<ObjectId> = from_value(attempt_answers).unwrap();

    let res = exam_utils::misc::compare_answers(&exam_answers,&generation_answers,&attempt_answers);

    to_value(res).unwrap()
}

#[wasm_bindgen]
pub fn validate_config(exam: JsValue) -> JsValue {
    let exam: prisma::ExamEnvironmentExam = from_value(exam).unwrap();

    let res = exam_utils::misc::validate_config(&exam);

    to_value(res).unwrap()
}

#[wasm_bindgen]
pub fn generate_exam(exam: JsValue) -> JsValue {
    let exam: exam_utils::misc::ExamInput = from_value(exam).unwrap();

    let res = exam_utils::misc::generate_exam(exam);

    to_value(res).unwrap()
}