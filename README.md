# Exam Services

- `exam-utils/`: useful utility functions working on exam data types
- `js-api/`: wasm bindings for `exam-utils`, published to npm as [`@freecodecamp/exam-services`](https://www.npmjs.com/package/@freecodecamp/exam-services)
- `moderation-service/`: tasks to manage exam attempts
- `prisma/`: prisma schema Rust export
- `script/`: assortment of past, once-off scripts interacting with the database

## Releases and deployment

[Release Please](https://github.com/googleapis/release-please) manages independent versions, changelogs, tags, and GitHub Releases for every Rust crate in this repository. Commits must use [Conventional Commits](https://www.conventionalcommits.org/): `fix` produces a patch, `feat` a minor, and `!`/`BREAKING CHANGE` a major release.

On each push to `main`, [`release.yml`](./.github/workflows/release.yml) opens or updates one release PR containing all affected packages. Merging that PR creates component tags such as `exam-utils-v3.2.0` and published GitHub Releases. Cargo workspace dependants and the root lockfile are updated together. Rust packages are not published to crates.io: this repository contains services/internal tools and its schema dependency chain includes unpublished Git dependencies.

Release side effects run from the exact tagged commit:

- `js-api` builds, smoke-tests, and publishes `@freecodecamp/exam-services` to npm with provenance
- `moderation-service` builds and pushes `<version>` and `latest` images to DOCR; DigitalOcean App Platform then auto-deploys the image

`deploy.yaml` and `publish-js-api.yml` remain manually runnable. Manual npm runs default to dry-run.

### Repository setup

1. In GitHub Actions settings, allow workflows read/write access and creation of pull requests.
2. Recommended: add `RELEASE_PLEASE_TOKEN` as a fine-grained PAT or GitHub App token with repository contents and pull-request write access. Automation falls back to `GITHUB_TOKEN`, but token-created release PRs do not trigger other workflows.
3. Create GitHub environment `npm`. Configure npm trusted publishing for package `@freecodecamp/exam-services` with organization `freeCodeCamp`, repository `exam-services`, workflow `publish-js-api.yml`, environment `npm`, and `npm publish` permission.
4. npm trusted publishing can only be configured after package exists. For first publication, add granular automation token `NPM_TOKEN` to `npm` environment and manually run `Publish JS API` with dry-run disabled. Configure trusted publishing afterward; then remove token.
5. Keep `DIGITALOCEAN_ACCESS_TOKEN` and `DOCR_NAME` repository secrets configured for moderation-service deployment.

## Moderation Service

### Development

```bash
docker build . --file ./moderation-service/Dockerfile
```

### Testing

Seed database with exam and attempt

```bash
cd freeCodeCamp/freeCodeCampm
pnpm run seed:exam-env --attempt
```

A specific version of `rustc` is used, because the test tooling requires nightly features.

```bash
cargo +nightly-2025-04-03 test
```

## Exam Utils

### Moderation score regression harness

`get_moderation_score` is high-risk. Every algorithm version is registered in
[`moderation_versions.rs`](./exam-utils/src/moderation_versions.rs) and run over a
committed, PII-free scenario catalog ([`testdata/moderation/scenarios/`](./exam-utils/testdata/moderation/scenarios/)).
Runs in CI ([`test-exam-utils.yml`](./.github/workflows/test-exam-utils.yml)); no nightly, no DB.

```bash
# Gate + report (report.html written to testdata/moderation/, git-ignored).
cargo test -p exam-utils
```

- **Gate**: `moderation_scores_golden` asserts scores match `scores.golden`. A change to a live version (or edit to a frozen one) fails it.
- **Report**: open `exam-utils/testdata/moderation/report.html` — per-version scores, deltas, threshold crossings. Includes real attempts if a local `fixtures/` dump exists.

Change the algorithm → review the drift → regenerate the golden:

```bash
UPDATE_GOLDEN=1 cargo test -p exam-utils moderation_scores_golden
git diff exam-utils/testdata/moderation/scores.golden
```

Add a case: drop a JSON file in `scenarios/`, then `UPDATE_GOLDEN=1`.
Add a version (when changing the algorithm): freeze the old body as `vN_<sha>` in `moderation_versions.rs`, append to `VERSIONS`, then `UPDATE_GOLDEN=1` — see that file's header.

## JS API

WASM bindings for `exam-utils`, consumable from Node.js and browsers. See [`js-api/README.md`](./js-api/README.md) for the API reference and consumption docs.

### Development

```bash
rustup target add wasm32-unknown-unknown
npm install -g wasm-pack

node js-api/build.mjs      # builds the npm package into js-api/pkg/
node js-api/smoke-test.mjs # runs the package against fixtures/ (when present)
```

### Releasing

Merge conventional commits that change `js-api/`. Release Please updates `js-api/Cargo.toml`, `js-api/Cargo.lock`, and `js-api/CHANGELOG.md`; merging its release PR creates the GitHub Release and publishes npm package. See [Releases and deployment](#releases-and-deployment).

## Script

```bash
cd script/
cargo run
```
