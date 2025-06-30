ARG BIN_NAME=moderation-service
ARG PROJECT_DIR=tools/${BIN_NAME}

FROM rust:1 AS chef
RUN cargo install cargo-chef
ARG PROJECT_DIR
WORKDIR /app

FROM chef AS planner
COPY ${PROJECT_DIR} ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG PROJECT_DIR
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY ${PROJECT_DIR} ./
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12 AS runtime
ARG BIN_NAME
COPY --from=builder /app/target/release/${BIN_NAME} /
ENTRYPOINT ["./moderation-service"]
