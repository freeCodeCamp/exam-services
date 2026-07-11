# Exam Services - JS API Changelog

## [0.2.0]

Published to npm as [`@freecodecamp/exam-services`](https://www.npmjs.com/package/@freecodecamp/exam-services) with Node.js (CommonJS) and web (ESM) builds.

- add `check_attempt_pass`, `calculate_score`, `compare_answers`, `validate_config`, and `generate_exam` bindings
- malformed arguments and util errors now throw JavaScript `Error`s (naming the offending argument) instead of aborting the wasm module
- `validate_generation` and `validate_config` return `string | undefined` instead of `string | null`
- output is JSON-compatible: 64-bit integers serialize as `number` instead of `BigInt`
- panics are logged as readable console errors via `console_error_panic_hook`
- add package build script (`build.mjs`), smoke test (`smoke-test.mjs`), and CI publish/test workflows

## [0.1.0]

initial release for testing
