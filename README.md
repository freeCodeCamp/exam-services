# Exam Services

```bash
cd moderation-service && cp sample.env .env
```

```bash
docker compose up -d
```

## Deployment

This project is deployed on the Digital Ocean App Platform as a Job on a schedule. The image is built using GitHub Actions, pushed to the Digital Ocean Container Registry, then the App Platform auto-deploys the new image.

To deploy:

1. manually run the `version-bump.yaml` GitHub Action workflow
2. a pull request will be created
3. approve the pull request
4. the `auto-release.yaml` workflow will cause `deploy.yaml` to build and push to DOCR

## Moderation Service

### Development

```bash
docker compose up -d moderation
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
