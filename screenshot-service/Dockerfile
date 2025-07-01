FROM rust:1 AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY ./ ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY ./ ./
RUN cargo build --release

FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=builder /app/target/release/screenshot-service /
ENTRYPOINT ["./screenshot-service"]
