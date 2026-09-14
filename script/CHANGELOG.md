# Exam Services - Script Changelog

Contains changes, as well as a record of bugs and log of runs for the one-off scripts.

See `README.md` for the process to follow when adding and running a script.

## [1.2.0](https://github.com/freeCodeCamp/exam-services/compare/script-v1.1.0...script-v1.2.0) (2026-09-14)


### Features

* add moderation score test harness and reporting ([9161b2d](https://github.com/freeCodeCamp/exam-services/commit/9161b2d4c0ad3de3d200b6d244370664d32f3235))

## [1.1.0]

- add `README.md`: conventions and flight manual for adding/running a fix script
- add `unset_challenges_awarded`

### Bug: `challengesAwarded` pre-set on auto-approved passing attempts

Introduced in `bc6af64` (`moderation-service` 3.2.0), reported after the release containing `68710305ee6d7bc319dcd0fb20d81cf470524b51`.

`update_moderation_collection` set `challengesAwarded: true` at record creation for passing attempts that auto-approved below the moderation threshold. `award_challenge_ids` builds its work queue from `{challengesAwarded: false, status: Approved}`, so those records were never processed and the users never received their `CompletedChallenge`.

Affected records are the cleanest submissions only - passing, below threshold. Failed attempts (correctly flagged) and above-threshold attempts (left `Pending`, flag untouched) were unaffected, so the symptom presented as intermittent rather than total.

Selector:

```
status:            Approved
challengesAwarded: true
feedback:          "Auto Approved"
version:           3
```

`feedback` excludes `"Auto Approved - Failed attempt"` and the pre-`bc6af64` `"Auto Approved - Moderation score: {n}"`. `version: 3` excludes records written before the bug shipped.

### Fix

Reset `challengesAwarded` to `false` on the affected records so `award_challenge_ids` reprocesses them. Re-awarding is idempotent - `award_challenge_ids` filters on `"completedChallenges.id": {"$ne": id}`.

#### Run log

| Date             | Environment | Documents reset | Notes |
| ---------------- | ----------- | --------------- | ----- |
| 07:39 06/08/2026 | staging     | 0               |       |
| 07:47 06/08/2026 | live        | 301             |       |
