# Exam Services - Moderation Service Changelog

## [3.2.0]

- bump `sentry` to `0.48.5`, enable `metrics` feature
- emit `exam_service.*` Sentry metrics (counters, gauge, distributions) for task and moderation stats
- remove per-item debug/info logs superseded by metrics

## [3.1.0]

- remove `temp_handle_duplicate_moderations` task

## [3.0.5]

- create moderation record on moderation score error

## [3.0.4]

- continue updating moderations on moderation score error

## [3.0.3]

- updated `exam-utils` to return an error when calculating moderation score instead of panicing
