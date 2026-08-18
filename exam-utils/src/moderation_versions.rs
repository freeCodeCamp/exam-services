//! Version registry for the moderation-score algorithm.
//!
//! `get_moderation_score` is high-risk: it decides whether an exam attempt is
//! auto-approved or flagged for manual review. Every time the formula changes,
//! the *previous* formula is frozen here as its own function so all versions can
//! be run side by side over the same inputs (see the `#[cfg(test)]`
//! `moderation_harness` module for the comparison report + regression gate).
//!
//! ## The registry
//!
//! [`VERSIONS`] lists every algorithm version oldest -> newest. Each entry has a
//! **stable, monotonic id**: `v2` names one formula forever, so a metric series,
//! a golden row, or a report column never changes meaning underneath you. There
//! is deliberately no floating `current`/`HEAD`/`latest` label - the live
//! function is registered under its own concrete number, and [`LIVE`] points at
//! that number.
//!
//! Every entry other than the live one is a FROZEN, byte-for-byte copy of a
//! retired formula and must never be "fixed" or refactored - its whole purpose
//! is to reproduce the old numbers exactly. Frozen functions are named for what
//! they *are* (`v1`), not for what replaced them; the commit that introduced
//! each formula is recorded in [`Version::introduced_in`].
//!
//! ## Adding a version (when you change `get_moderation_score`)
//!
//! 1. Copy the *current* body of `get_moderation_score` into a new frozen `vN`
//!    function below, verbatim, with a DO-NOT-REFACTOR note.
//! 2. Flip the previously-live entry to `state: State::Frozen` and repoint its
//!    `score` at that new frozen `vN`.
//! 3. Append a new entry `id: N + 1`, `label: "v<N+1>"`, `introduced_in: <the
//!    commit you are about to make>`, `score: crate::attempt::get_moderation_score`,
//!    `state: State::Live`, and bump [`LIVE`] to `N + 1`. Never renumber or
//!    reuse an id.
//! 4. Edit `get_moderation_score`.
//! 5. Regenerate the golden: `UPDATE_GOLDEN=1 cargo test -p exam-utils
//!    moderation_scores_golden`, then review the `scores.golden` diff.

use prisma::supabase::{Event, EventKind};

use crate::attempt::Attempt;
use crate::error::Error;

/// A moderation-score algorithm version: same signature as the live function.
pub type ScoreFn = fn(&Attempt, &Vec<Event>) -> Result<f64, Error>;

/// Whether a registered version is retired or the one currently in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Retired formula, kept verbatim for comparison. Never edit.
    Frozen,
    /// The formula whose score drives auto-approval today.
    Live,
}

/// One moderation-score algorithm version.
#[derive(Debug, Clone, Copy)]
pub struct Version {
    /// Stable and monotonic. Never reused, never renumbered - a persisted or
    /// reported `id` must always resolve to the same formula.
    pub id: u32,
    /// Metric/report/golden label. Always `v{id}`.
    pub label: &'static str,
    /// Commit that introduced this formula.
    pub introduced_in: &'static str,
    pub score: ScoreFn,
    pub state: State,
}

/// Every moderation-score version, oldest -> newest. See the module docs for how
/// to append a version.
pub const VERSIONS: &[Version] = &[
    Version {
        id: 1,
        label: "v1",
        introduced_in: "initial",
        score: v1,
        state: State::Frozen,
    },
    Version {
        id: 2,
        label: "v2",
        introduced_in: "bc6af64",
        score: crate::attempt::get_moderation_score,
        state: State::Live,
    },
];

/// Id of the version whose score drives auto-approval. Must match the single
/// [`State::Live`] entry in [`VERSIONS`] (asserted by [`live`]).
pub const LIVE: u32 = 2;

/// The live version. Panics if the registry and [`LIVE`] disagree, which is a
/// programming error in this file.
pub fn live() -> &'static Version {
    let version = VERSIONS
        .iter()
        .find(|v| v.id == LIVE)
        .expect("LIVE id must be registered in VERSIONS");
    assert_eq!(
        version.state,
        State::Live,
        "LIVE id {LIVE} is registered as frozen"
    );
    version
}

/// Look up a version by its stable id.
pub fn by_id(id: u32) -> Option<&'static Version> {
    VERSIONS.iter().find(|v| v.id == id)
}

// ============================================================================
// FROZEN - v1, copied verbatim from exam-utils/src/attempt.rs @ bc6af64^
// (before commit bc6af64 "fix: update deps and schema"). Previously duplicated
// in script/src/bin/generate_prev_moderation_scores.rs; this is now the single
// source of truth.
//
// Diff vs v2: v2 drops `blur_weight` (total_blur_time / total_time) and the
// `total_blur_time_before_last_answer > total_time_taken` guard, and replaces
// the linear time_weight + blur-time terms with a fast-completion term, a
// per-question median term, and a blurred-question-count term.
//
// Do NOT "fix" or refactor this - it must stay frozen at the old behaviour so
// the regression report can reproduce v1's exact numbers.
//
// `pub` so `script/src/bin/generate_prev_moderation_scores.rs` can reuse it
// instead of keeping its own second copy.
// ============================================================================
pub fn v1(attempt: &Attempt, events: &Vec<Event>) -> Result<f64, Error> {
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
            return Ok(moderation_score);
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

    if total_time_taken > total_time {
        return Err(Error::ModerationScore(format!(
            "total time taken > than total time: {} > {}",
            total_time_taken, total_time
        )));
    }
    if total_blur_time > total_time {
        return Err(Error::ModerationScore(format!(
            "total blur time > total time: {total_blur_time} > {total_time}"
        )));
    }
    if total_blur_time_before_last_answer > total_blur_time {
        return Err(Error::ModerationScore(format!(
            "total blur time before last answer > total blur time: {total_blur_time_before_last_answer} > {total_blur_time}"
        )));
    }

    let time_weight = ((total_time - total_time_taken) / total_time) * weight;
    if time_weight > weight {
        return Err(Error::ModerationScore(format!(
            "time weight > weight: {time_weight} > {weight}"
        )));
    }
    moderation_score += time_weight;

    // Blur time after last submission is worth 1/3 as much as before last submission
    // Seeing as both total_blur_time_* vars include the time before, it is counted 'twice'
    let blur_weight = (total_blur_time / total_time) * weight;
    if blur_weight > weight {
        return Err(Error::ModerationScore(format!(
            "blur weight > weight: {blur_weight} > {weight}"
        )));
    }
    moderation_score += blur_weight;

    if total_blur_time_before_last_answer > total_time_taken {
        return Err(Error::ModerationScore(format!(
            "total blur time before last answer > total time taken: {total_blur_time_before_last_answer} > {total_time_taken}"
        )));
    }

    let blur_before_weight = (total_blur_time_before_last_answer / total_time_taken) * weight * 2.0;
    if blur_before_weight > weight * 2.0 {
        return Err(Error::ModerationScore(format!(
            "blur before weight > weight: {blur_before_weight} > {}",
            weight * 2.0
        )));
    }
    moderation_score += blur_before_weight;

    if moderation_score > 1.0 {
        tracing::error!(
            attempt = %attempt.id,
            moderation_score,
            "moderation score should never be > 1.0"
        );
    }

    Ok(moderation_score)
}

#[cfg(test)]
mod registry {
    use super::*;

    /// Guards the registry's two invariants: ids are strictly increasing (so
    /// oldest -> newest ordering is real and no id is reused), and labels are
    /// mechanically derived from ids (so a label cannot drift off its formula).
    #[test]
    fn ids_are_monotonic_and_labels_match() {
        for (i, version) in VERSIONS.iter().enumerate() {
            assert_eq!(
                version.label,
                format!("v{}", version.id),
                "label must be v{{id}}"
            );
            if i > 0 {
                let previous = &VERSIONS[i - 1];
                assert!(
                    version.id > previous.id,
                    "VERSIONS must be ordered oldest -> newest by id: {} after {}",
                    version.id,
                    previous.id
                );
            }
        }
    }

    /// Exactly one version is in force, and `LIVE` names it.
    #[test]
    fn exactly_one_live_version() {
        let live_versions: Vec<u32> = VERSIONS
            .iter()
            .filter(|v| v.state == State::Live)
            .map(|v| v.id)
            .collect();
        assert_eq!(live_versions, vec![LIVE]);
        assert_eq!(live().id, LIVE);
    }
}
