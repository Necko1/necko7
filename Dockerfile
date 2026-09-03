# 1. Base chef image with build tools
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    clang \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# 2. Planner stage - compute dependency recipe
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# 3. Builder stage - cook dependencies and compile binary
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Cook dependencies - cached layer as long as Cargo.toml and Cargo.lock are unchanged
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source code and embedded migrations
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

# Compile binary and strip debug symbols to minimize image footprint
RUN cargo build --release --bin necko7 && \
    strip /app/target/release/necko7

# 4. Minimal runtime stage
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/necko7 /usr/local/bin/necko7

# Create non-root user for security
RUN groupadd -g 1000 necko && useradd -u 1000 -g necko -s /bin/false necko
USER necko

EXPOSE 8080
ENV RUST_LOG=info \
    BIND_ADDR=0.0.0.0:8080

ENTRYPOINT ["/usr/local/bin/necko7"]
