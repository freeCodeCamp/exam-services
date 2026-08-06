# Exam Services - Script

One-off scripts that read and mutate production data directly. There is no review gate between this crate and the live `freecodecamp` database, so the process below is the gate.

Every script here exists because a bug already reached production. Treat each run as a second chance to make the same mistake at scale.

## Layout

- `src/main.rs` - swap-in harness. Declares exactly one `mod`, calls exactly one script.
- `src/*.rs` - the scripts. Retired ones keep an `_` name prefix and are not declared in `main.rs`, so they do not compile and cannot be run by accident.
- `CHANGELOG.md` - the bug record and the run log. Not optional; see step 9.

This crate is its own cargo workspace. Run cargo from inside `script/`.

## Conventions

| Concern     | Convention                                                     |
| ----------- | -------------------------------------------------------------- |
| Entry point | `pub async fn <name>(client: Client) -> Result<(), String>`    |
| Safety      | Dry run by default. `CONFIRM=1` applies writes.                |
| Connection  | `MONGODB_URI` from `script/.env` (gitignored)                  |
| Audit       | Per-record `info!` with the ids touched; lands in `logs.jsonl` |
| Doc comment | Bulleted description of what it selects and what it writes     |

## Flight manual

### 1. Pin the root cause before writing a selector

Identify the commit and the exact line. A backfill selector is only as good as your understanding of which records the bug touched - if you cannot name the commit, you are guessing at the blast radius.

### 2. Make the selector discriminating, and prove it

Find a field value unique to the buggy code path, and gate on a second independent field as a belt-and-braces check.

Write down what the selector must _exclude_ and confirm each exclusion holds.

### 3. Order the backfill against the code fix

Decide this deliberately - it is the step most easily missed.

- If the selector keys on a string the fix changes, **backfill before deploying the fix**, or the records become unidentifiable.
- If the bug keeps producing bad records, **deploy the fix first**, or you backfill into a moving target.

If both apply, widen the selector to something stable across the fix (a prefix match, a date bound) before deploying anything.

### 4. Make it idempotent, or make it impossible to run twice

Assume the script will be run twice - interrupted midway, or by someone reading the changelog a year from now. Prefer writes that are no-ops on a second pass (`$ne` guards, `$addToSet`, setting a flag to a fixed value rather than toggling it).

State the idempotency argument in the doc comment. If you cannot make one, say so there.

### 5. Pause the scheduled job if the script races it

`moderation-service` runs as a scheduled DigitalOcean App Platform Job against the same collections. A backfill that flips a flag the service reads is racing it.

Either pause the job for the duration, or collect ids in a read pass and write by explicit `{"_id": {"$in": ids}}` so records created mid-run cannot be swept in. The read-then-write-by-id pattern is what `unset_challenges_awarded` uses.

### 6. Dry run, and read the output

```bash
cd script
cargo run          # dry run: prints the count, logs every affected id
```

Sanity-check the count against the incident. An order of magnitude off in either direction means the selector is wrong - stop and go back to step 2.

**`main.rs` truncates `logs.jsonl` on every run.** Copy it aside before the real run, or the dry-run audit trail is destroyed by the write you are auditing:

```bash
cp logs.jsonl logs.dry-run.jsonl
```

### 7. Apply

```bash
CONFIRM=1 cargo run
cp logs.jsonl logs.applied.jsonl
```

Keep both log files until the fix is verified. They are the only record of which documents changed.

### 8. Verify against the user-visible symptom

Not against the field you just wrote. The flag flip is the mechanism; the fix is a user holding a `CompletedChallenge`. Pick a handful of ids from `logs.applied.jsonl`, follow them to the user documents, and confirm the outcome. Check the corresponding `exam_service.*` Sentry counters moved.

### 9. Record the run in `CHANGELOG.md`

The changelog is the institutional memory for this crate - what broke, what was run against production, when, and how many documents moved. Record:

- the bug, the commit that introduced it, and the affected record shape
- the selector used
- the date of the run, the environment, and the document count

### 10. Retire the script

Rename the function and file with an `_` prefix and remove the `mod` line from `main.rs`. It stays in the repo as a reference; it stops being a loaded gun.

## Checklist

```
[ ] Root-cause commit identified
[ ] Selector discriminating; exclusions verified
[ ] Backfill ordered against the code fix deliberately
[ ] Idempotency argued in the doc comment
[ ] Race with the scheduled job handled
[ ] Dry run count sane; logs.jsonl copied aside
[ ] Applied with CONFIRM=1; logs copied aside
[ ] Verified against the user-visible symptom
[ ] CHANGELOG.md updated with bug + run log
[ ] Script retired (`_` prefix, mod removed)
```
