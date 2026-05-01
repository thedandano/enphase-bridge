FROM rust:1.90-slim AS builder
WORKDIR /app

# Cache dependency compilation separately from source
COPY Cargo.toml Cargo.lock ./
RUN mkdir src src/bin \
    && echo 'fn main(){}' > src/main.rs \
    && echo '' > src/lib.rs \
    && echo 'fn main(){}' > src/bin/recompute_windows.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/enphase-bridge .
COPY migrations ./migrations
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fs --max-time 3 "http://localhost:${ENPHASE__API__PORT:-8080}/api/health" || exit 1
CMD ["./enphase-bridge"]
