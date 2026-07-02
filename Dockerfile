# ── Stage 1: Build ─────────────────────────────────────────────────────────
# Optional containerized build (cloud fallback / local dev). Production runs on
# bare metal via deploy/kingfisher.service with `cargo build --release --features ipc`.
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifest first for layer caching.
# Cargo.lock is copied if present (glob tolerates its absence); cargo generates one
# on first build otherwise. Committing a Cargo.lock is recommended for reproducible
# builds — generate it with `cargo generate-lockfile` on a machine with network access.
COPY bot/Cargo.toml bot/Cargo.loc[k] ./bot/
COPY bot/bin/Cargo.toml           ./bot/bin/
COPY bot/crates/core/Cargo.toml   ./bot/crates/core/
COPY bot/crates/chain/Cargo.toml  ./bot/crates/chain/
COPY bot/crates/scanner/Cargo.toml ./bot/crates/scanner/
COPY bot/crates/simulation/Cargo.toml ./bot/crates/simulation/
COPY bot/crates/edges/Cargo.toml  ./bot/crates/edges/
COPY bot/crates/executor/Cargo.toml ./bot/crates/executor/
COPY bot/crates/api/Cargo.toml    ./bot/crates/api/

# Stub src files for dependency caching layer
RUN mkdir -p bot/bin/src \
             bot/crates/core/src \
             bot/crates/chain/src \
             bot/crates/scanner/src \
             bot/crates/simulation/src \
             bot/crates/edges/src \
             bot/crates/executor/src \
             bot/crates/api/src \
    && echo "fn main(){}" > bot/bin/src/main.rs \
    && for c in core chain scanner simulation edges executor api; do \
         echo "" > bot/crates/$c/src/lib.rs; done

WORKDIR /app/bot
RUN cargo build --release --bin kingfisher 2>/dev/null || true

# Now copy real source
COPY bot/ /app/bot/
RUN touch /app/bot/bin/src/main.rs \
    && cargo build --release --bin kingfisher

# ── Stage 2: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/bot/target/release/kingfisher /usr/local/bin/kingfisher

# Health check via curl
HEALTHCHECK --interval=10s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:3001/health || exit 1

EXPOSE 3001

CMD ["kingfisher"]
