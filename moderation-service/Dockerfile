FROM rust:1 AS builder
WORKDIR /app
ARG BIN_NAME
COPY . .
RUN cargo build --release

# FROM gcr.io/distroless/cc-debian12 AS runtime
FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends openssl ca-certificates \
    # Clean up
    && apt-get autoremove -y \
    && apt-get clean -y \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/moderation-service moderation-service
ENTRYPOINT ["./moderation-service"]
