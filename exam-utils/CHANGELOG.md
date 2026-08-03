# Exam Services - Exam Utils Changelog

## [3.1.0]

- rewrite `get_moderation_score`: time-taken and median-time-per-question weighting, blur periods tracked per-question instead of as running totals
- add public helpers backing the new algorithm: `get_time_per_question`, `get_total_time`, `get_blur_periods`, `get_total_blur_time`, `get_total_blur_time_before_last_answer`, `get_last_submission_time`, and `QuestionBlurPeriods`/`Period` types
- add `moderation_versions` module exposing prior algorithm versions (e.g. `v1_pre_bc6af64`) for side-by-side comparison
- add `moderation_harness` test harness: runs each `moderation_versions` entry over a committed scenario catalog and gates scores against a golden file
- enable `bson` `chrono-0_4` feature

## [3.0.0]

- fix `generate_exam` producing generations with duplicate question set / question ids:
  - allocated questions were not removed from the pool when a config was fulfilled mid-iteration
  - question sets used by multiple question set configs of the same type are now merged in the output
- `ExamInput` accepts `_id` as an alias for `id`, so a full exam document can be passed as-is
- support `wasm32-unknown-unknown` builds (for `js-api`):
  - remove `mongodb` dependency (use `bson` directly)
  - remove unused `Error` variants (`MongoDB`, `SystemTimeError`, `BsonAccess`, `BsonSerialization`)
  - use `web-time` instead of `std::time` for the generation timeout

## [2.0.0]

- return error instead of panic when calculating moderation score
