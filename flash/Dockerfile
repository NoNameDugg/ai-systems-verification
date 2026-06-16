# =============================================================================
# Flash - Rust High-Performance Market Data Engine
# Multi-Stage Docker Build
# =============================================================================
#
# Target: sub-millisecond P99 latency (see README for benchmark caveats)
#
# Build:
#   docker build -t astra-flash:latest .
#
# Run:
#   docker run -d \
#     --name astra-flash \
#     -e OANDA_API_TOKEN=$OANDA_API_TOKEN \
#     -e OANDA_ACCOUNT_ID=$OANDA_ACCOUNT_ID \
#     -e ASTRA_FLASH_REDIS_URL=redis://redis:6379 \
#     -v ./config:/app/config:ro \
#     -p 9090:9090 \
#     astra-flash:latest
#
# =============================================================================

# ============================================================================
# BUILD STAGE - Rust Compilation
# ============================================================================
FROM rust:1.85-slim-bookworm AS builder

LABEL stage="builder"
LABEL description="Flash Rust Builder"

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy Cargo manifests first (for layer caching)
COPY Cargo.toml Cargo.lock ./

# Create dummy main.rs for dependency caching
RUN mkdir -p src && \
    echo 'fn main() { println!("Dummy"); }' > src/main.rs && \
    echo 'pub fn dummy() {}' > src/lib.rs

# Build dependencies only (cached layer)
RUN cargo build --release 2>/dev/null || true

# Remove dummy source
RUN rm -rf src

# Copy actual source code
COPY src ./src
COPY benches ./benches
COPY tests ./tests

# Build the application with optimizations
RUN RUSTFLAGS="-C target-cpu=native" cargo build --release --bin astra-flash

# Verify the binary was built
RUN test -f /build/target/release/astra-flash && \
    echo "Binary size: $(du -h /build/target/release/astra-flash | cut -f1)"

# ============================================================================
# RUNTIME STAGE - Minimal Production Image
# ============================================================================
FROM debian:bookworm-slim AS runtime

# Labels for container identification
LABEL org.opencontainers.image.title="Flash"
LABEL org.opencontainers.image.description="High-performance Rust market data engine"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.licenses="MIT"

# Application environment
ENV ASTRA_ENV=production
ENV ASTRA_LOG_LEVEL=INFO
ENV APP_HOME=/app
ENV RUST_BACKTRACE=1

# Configuration paths
ENV ASTRA_FLASH_CONFIG=/app/config/flash.yaml

# Install runtime dependencies only
# - ca-certificates: Required for HTTPS (OANDA API)
# - libssl3: Required for TLS connections
# - curl: Required for healthcheck
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

WORKDIR ${APP_HOME}

# Copy the compiled binary from builder
COPY --from=builder /build/target/release/astra-flash ./astra-flash

# Copy configuration files
COPY config ./config

# Create data directory for any runtime files
RUN mkdir -p ./data ./logs

# ============================================================================
# USER SETUP (Non-root for security)
# ============================================================================
RUN useradd --create-home --shell /bin/bash --uid 1000 astra \
    && chown -R astra:astra ${APP_HOME}

USER astra

# ============================================================================
# EXPOSE PORTS
# ============================================================================
# 9090: Prometheus metrics endpoint
EXPOSE 9090

# ============================================================================
# HEALTHCHECK
# ============================================================================
# Use metrics endpoint for health verification
# The /metrics endpoint is served when Flash is running
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD curl -sf http://localhost:9090/metrics || exit 1

# ============================================================================
# ENTRYPOINT
# ============================================================================
# Run the Flash binary with the config path
ENTRYPOINT ["./astra-flash"]
CMD []
