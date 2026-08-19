# @freecodecamp/exam-services

WASM bindings for the [freeCodeCamp exam-services](https://github.com/freeCodeCamp/exam-services) utilities: exam generation, scoring, validation, and moderation scoring. Runs in Node.js and browsers.

## Install

```bash
npm install @freecodecamp/exam-services
```

## Usage

### Node.js

The Node build loads synchronously (CommonJS, usable from ESM):

```js
import { generate_exam, calculate_score } from "@freecodecamp/exam-services";

const generated = generate_exam(exam);
const score = calculate_score(exam, generated, attempt);
```

### Browser

The web build is ESM and must be initialized once before use:

```js
import init, { generate_exam } from "@freecodecamp/exam-services/web";

await init();
const generated = generate_exam(exam);
```

`init()` fetches `exam_services_bg.wasm` relative to the module URL. Bundlers that do not handle this automatically can pass the wasm URL explicitly, e.g. Vite:

```js
import init from "@freecodecamp/exam-services/web";
import wasmUrl from "@freecodecamp/exam-services/web/exam_services_bg.wasm?url";

await init({ module_or_path: wasmUrl });
```

## Data shapes

Arguments and return values mirror the MongoDB extended JSON representation of the [Prisma models](https://github.com/freeCodeCamp/freeCodeCamp/blob/main/api/prisma/exam-environment.prisma) (`ExamEnvironmentExam`, `ExamEnvironmentGeneratedExam`, `ExamEnvironmentExamAttempt`):

- `ObjectId`: `{ "$oid": "686fcea76693f5a009aa031f" }`
- `DateTime`: `{ "$date": { "$numberLong": "1770046619980" } }`
- Field names are camelCase (`questionSets`, `passingPercent`, ...), documents use `_id`

Exceptions:

- `Attempt.startTime` (returned by `construct_attempt`) is an RFC3339 string
- Proctoring `Event`s use the Supabase shape: `{ id: string, timestamp: string (RFC3339), kind: "FOCUS" | "BLUR" | "QUESTION_VISIT" | "CAPTIONS_OPENED" | "EXAM_EXIT", meta: object, attempt_id: { "$oid": string } }`

See [`fixtures/`](https://github.com/freeCodeCamp/exam-services/tree/main/fixtures) for real examples of every shape.

## API

All functions throw a JavaScript `Error` when an argument does not deserialize into the expected shape. The error message names the offending argument.

### `generate_exam(exam): GeneratedExam`

Generates a randomized exam (question selection, answer subsetting, shuffling) satisfying `exam.config`. Only `_id` (or `id`), `questionSets`, and `config` are used. Throws if the config cannot be satisfied.

### `validate_config(exam): string | undefined`

Checks that the exam config is solvable (enough question sets, questions, tags, correct/incorrect answers; non-empty texts). Returns the reason as a string when invalid, otherwise `undefined`.

### `validate_generation(generation): string | undefined`

Checks a generated exam for duplicate question set / question / answer ids. Returns the reason as a string when invalid, otherwise `undefined`.

### `calculate_score(exam, generation, attempt): number`

Percentage (`0.0`–`100.0`) of generated questions answered correctly. Throws if the attempt references questions missing from the exam or generation.

### `check_attempt_pass(exam, generation, attempt): boolean`

`calculate_score(...) >= exam.config.passingPercent`. Returns `false` when the score cannot be calculated.

### `compare_answers(examAnswers, generationAnswers, attemptAnswers): boolean`

Single-question check: `true` when exactly the correct shown answers were selected. `examAnswers` is the full `answers` array (with `isCorrect`); the other two are arrays of `ObjectId`.

### `construct_attempt(exam, generation, attempt): Attempt`

Combines the three documents into one self-contained `Attempt`: exam questions annotated with `generated` (shown) answer ids, `selected` answer ids, and `submissionTime`.

### `get_moderation_score(attempt, events): number`

`0.0` (definitely no moderation needed) to `1.0` (definitely needs moderation), based on completion time and window blur/focus events. Takes an `Attempt` from `construct_attempt`. Throws when event timings are inconsistent with the attempt.

### `init_panic_hook(): void`

Runs automatically on instantiation; forwards Rust panics to `console.error`. Never needs to be called manually.

## Building locally

```bash
rustup target add wasm32-unknown-unknown
npm install -g wasm-pack

node js-api/build.mjs      # builds pkg/ (node + web targets)
node js-api/smoke-test.mjs # runs pkg/node against fixtures/
```

## Releasing

Use Conventional Commits for changes under `js-api/`. Release Please updates version and changelog in its release PR. Merging that PR creates `js-api-v<version>` GitHub Release, then [`publish-js-api.yml`](https://github.com/freeCodeCamp/exam-services/blob/main/.github/workflows/publish-js-api.yml) builds, smoke-tests, and publishes to npm through trusted publishing with provenance. Manual workflow runs default to dry-run.

See repository [release setup](../README.md#repository-setup) for npm trusted-publisher and first-publication configuration.
