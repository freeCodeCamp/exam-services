use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

#[wasm_bindgen]
pub fn validate_generation(val: JsValue) -> JsValue {
    let generation: prisma::ExamEnvironmentGeneratedExam = from_value(val).unwrap();
    let res = exam_utils::generation::validate_generation(&generation);

    if let Err(e) = res {
        return to_value(&e.to_string()).unwrap();
    }

    JsValue::null()
}
