# syntax=docker/dockerfile:1

# ============================================================
# Stage 1: Planner — generate cargo-chef recipe for caching
# ============================================================
FROM rust:1.96.0-slim-bookworm AS planner
WORKDIR /app
RUN cargo install cargo-chef --locked
# Copy manifests AND source (cargo metadata needs src/ to discover crate targets)
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo chef prepare --recipe-path recipe.json

# ============================================================
# Stage 2: Builder — compile release binary
# ============================================================
FROM rust:1.96.0-slim-bookworm AS builder
WORKDIR /app
RUN cargo install cargo-chef --locked
# Build dependencies only (cached until Cargo.toml / Cargo.lock change)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
# Copy source and migrations (sqlx embeds migrations at compile time)
COPY src/ src/
COPY migrations/ migrations/
# Build the application
RUN cargo build --release --locked && \
    cp target/release/api-manage-platform /app/api-manage-platform

# ============================================================
# Stage 3: Runtime — minimal production image
# ============================================================
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
# - ca-certificates: TLS verification for PostgreSQL and Valkey connections
# - curl: used by HEALTHCHECK
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user (no shell, no login)
RUN groupadd -r appuser && \
    useradd -r -g appuser -d /app -s /sbin/nologin appuser

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/api-manage-platform /app/api-manage-platform

# Copy config directory (required at runtime by config::load())
COPY config/ config/

# Create log directory and set ownership
RUN mkdir -p logs && \
    chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Override default host for container (0.0.0.0 instead of 127.0.0.1)
ENV APP__SERVER__HOST=0.0.0.0

EXPOSE 3000

# Health check using the app's own /api/v1/health endpoint
HEALTHCHECK --interval=30s --timeout=5s --start-period=40s --retries=3 \
    CMD curl --fail http://localhost:3000/api/v1/health || exit 1

ENTRYPOINT ["/app/api-manage-platform"]
