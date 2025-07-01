# Exam Services

```bash
cd moderation-service && cp sample.env .env
cd screenshot-service && cp sample.env .env
```

```bash
docker compose up -d
```

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

## Screenshot Service

### Development

```bash
docker compose up -d screenshot
```

## Design Philosophy

Sentry error events are emitted when things that should not go wrong, go wrong.

Sentry traces (transactions) are emitted to log things, and to show errors that are allowed to happen.
