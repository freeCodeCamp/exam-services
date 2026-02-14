# Exam Services

- `exam-utils/`: useful utility functions working on exam data types
- `js-api/`: wasm bindings for `exam-utils`
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

## Script

```bash
cd script/
cargo run
```
