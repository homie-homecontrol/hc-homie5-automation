# Build Stage 1 - Dependency Cache
FROM rust:1.85-bookworm AS chef

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /service/hc-homie5-automation/

# Install cargo-chef for caching dependencies
RUN cargo install cargo-chef

# Copy only Cargo manifest files (no source code)
COPY Cargo.toml Cargo.lock ./

# Generate cargo recipe (snapshot of dependencies)
RUN cargo chef prepare --recipe-path recipe.json

# Build Stage 2 - Build Dependencies (Cached)
FROM rust:1.85-bookworm AS builder-deps

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /service/hc-homie5-automation/

# Install cargo-chef
RUN cargo install cargo-chef

# Copy the dependency recipe and cargo files
COPY --from=chef /service/hc-homie5-automation/recipe.json recipe.json
COPY Cargo.toml Cargo.lock ./

# Cache dependency compilation
RUN cargo chef cook --release --recipe-path recipe.json

# Build Stage 3 - Final Build with Application Source
FROM rust:1.85-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /service/hc-homie5-automation/

# Copy cached dependencies from the previous stage
COPY --from=builder-deps /usr/local/cargo /usr/local/cargo
COPY --from=builder-deps /service/hc-homie5-automation/target target

# Copy actual source code
COPY . .

# Build Rust application
RUN cargo build --release && \
    strip target/release/hc-homie5-automation  # Strip debug symbols

# Final Image - Minimal Runtime
FROM debian:bookworm-slim AS runtime

# Install only runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /service

# Copy compiled binary
COPY --from=builder /service/hc-homie5-automation/target/release/hc-homie5-automation /service/

# Prepare required directories
RUN mkdir -p /service/rules /service/virtual_devices && \
    chown -R appuser:appuser /service

# Environment variables
ENV HCACTL_HOMIE_HOST="mqtt" \
    HCACTL_HOMIE_CLIENT_ID="hcactl-1" \
    HCACTL_HOMIE_DOMAIN="homie" \
    HCACTL_HOMIE_USERNAME="" \
    HCACTL_HOMIE_PASSWORD="" \
    HCACTL_HOMIE_CTRL_ID="dev-autoctl-1" \
    HCACTL_VIRTUAL_DEVICES_CONFIG="file:/service/virtual_devices" \
    HCACTL_RULES_CONFIG="file:/service/rules" \
    HCACTL_VALUE_STORE_CONFIG="inmemory" \
    HCACTL_LOCATION="0.0,0.0,0.0"

# Switch to non-root user
USER appuser

ENTRYPOINT ["/service/hc-homie5-automation"]
