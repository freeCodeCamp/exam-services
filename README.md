# Exam Services

```bash
cd moderation-service && cp sample.env .env
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
