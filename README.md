# Exam Services

## Development

To run:

```bash
cargo run
```

## Deployment

Build the Docker image:

```bash
docker build -t exam-moderation-service -f ./docker/exam-moderation-service/Dockerfile .
```

Run the Docker container:

```bash
docker run -d exam-moderation-service
```

## Testing

Seed database with exam and attempt

```bash
pnpm run seed:exam-env --attempt
```

A specific version of `rustc` is used, because the test tooling requires nightly features.

```bash
cargo +nightly-2025-04-03 test
```

## Design Philosophy

Sentry error events are emitted when things that should not go wrong, go wrong.

Sentry traces (transactions) are emitted to log things, and to show errors that are allowed to happen.
