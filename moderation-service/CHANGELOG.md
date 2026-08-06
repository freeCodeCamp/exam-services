# Exam Services - Moderation Service Changelog

## [3.3.1]

- remove `challenges_awarded = true` from moderation score calculation

## [3.3.0]

- run new `exam-utils` moderation score algorithm alongside legacy (`v1_pre_bc6af64`) for comparison
- store new-algorithm result in `moderation_score`; legacy score still gates the auto-approval decision
- emit `exam_service.moderation_score_duration` Sentry metric per algorithm version
- bump `sentry` to `0.49.0`, `tokio` to `1.53.1`

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
