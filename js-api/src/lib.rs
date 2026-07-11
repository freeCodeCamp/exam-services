//! WASM bindings for the exam-services utilities.
//!
//! All functions take and return plain JavaScript values. Object shapes match
//! the MongoDB extended JSON representation of the Prisma models, e.g.
//! `ObjectId` is `{ "$oid": "..." }`.
//!
//! Errors are thrown as JavaScript `Error`s.
pub mod attempt;
pub mod generation;
pub mod misc;

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Runs automatically on instantiation: panics (bugs) surface as readable
/// console errors instead of `RuntimeError: unreachable`.
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Deserializes a JS value, prefixing errors with the argument name.
pub(crate) fn parse<T: serde::de::DeserializeOwned>(
    arg: &str,
    value: JsValue,
) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(value).map_err(|e| JsError::new(&format!("{arg}: {e}")))
}

/// Serializes into JSON-compatible JS values: maps become plain objects,
/// 64-bit integers become numbers (not BigInt), `None` becomes `null`.
pub(crate) fn serialize<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    value
        .serialize(&serializer)
        .map_err(|e| JsError::new(&e.to_string()))
}
