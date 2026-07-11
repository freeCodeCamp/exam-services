# Exam Services

- `exam-utils/`: useful utility functions working on exam data types
- `js-api/`: wasm bindings for `exam-utils`, published to npm as [`@freecodecamp/exam-services`](https://www.npmjs.com/package/@freecodecamp/exam-services)
- `moderation-service/`: tasks to manage exam attempts
- `prisma/`: prisma schema Rust export
- `script/`: assortment of past, once-off scripts interacting with the database

## Deployment

This project is deployed on the Digital Ocean App Platform as a Job on a schedule. The image is built using GitHub Actions, pushed to the Digital Ocean Container Registry, then the App Platform auto-deploys the new image.

To deploy:

1. manually bump the version(s) of the changed package(s)
2. run the `deploy.yaml` to build and push to DOCR

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

1. bump `version` in `js-api/Cargo.toml` and update `js-api/CHANGELOG.md`
2. commit, then tag and push: `git tag js-api-v<version> && git push origin js-api-v<version>`

The `publish-js-api.yml` workflow (also runnable via `workflow_dispatch`) builds, smoke-tests, and publishes to npm with provenance. Requires the `NPM_TOKEN` repository secret.

## Script

```bash
cd script/
cargo run
```
