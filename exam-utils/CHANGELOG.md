# Exam Services - Exam Utils Changelog

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
