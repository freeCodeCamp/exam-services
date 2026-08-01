//! Exam Environment Utility Functions
//!
//! ## Current API
//!
//! - Calculate attempt score
//! - Validate exam config
//! - Generate exams
//! - Validate generated exams
//!
pub mod attempt;
pub mod error;
pub mod generation;
pub mod misc;
pub mod moderation_versions;

/// Version-comparison harness: runs every [`moderation_versions::VERSIONS`] entry
/// over a committed synthetic scenario catalog, gates the scores against a golden
/// file, and renders an HTML comparison report.
#[cfg(test)]
mod moderation_harness;
