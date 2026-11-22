# Build from the repo root: docker build -f frontdoor/Dockerfile .
FROM --platform=linux/arm64 rust:1-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
RUN apt-get update && apt-get install -y --no-install-recommends build-essential && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p ts2-frontdoor

FROM --platform=linux/arm64 debian:bookworm-slim
COPY --from=builder /app/target/release/ts2-frontdoor /usr/local/bin/ts2-frontdoor
EXPOSE 9000
ENTRYPOINT ["ts2-frontdoor", "/etc/ts2/frontdoor.yaml"]
