# ARG to detect platform during multi-arch builds
ARG TARGETARCH

# Base Build Stage - Setup Dependencies (using prebuilt cargo-chef)
FROM rust:1.85-bookworm AS chef

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Detect correct cargo-chef binary
RUN ARCH=$([ "$TARGETARCH" = "amd64" ] && echo "x86_64" || ([ "$TARGETARCH" = "arm64" ] && echo "aarch64" || echo "armv7")) && \
    curl -L -o /usr/local/bin/cargo-chef \
    https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.67/cargo-chef-v0.1.67-${ARCH}-unknown-linux-gnu && \
    chmod +x /usr/local/bin/cargo-chef

# Set working directory
WORKDIR /service/hc-homie5-automation/

# Copy only Cargo manifest files (no source code yet)
COPY Cargo.toml Cargo.lock ./

# Generate recipe.json (dependency snapshot)
RUN cargo-chef prepare --recipe-path recipe.json

# Build Stage - Build Dependencies (cached layer for deps only)
FROM rust:1.85-bookworm AS builder-deps

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Detect correct cargo-chef binary (same as before)
RUN ARCH=$([ "$TARGETARCH" = "amd64" ] && echo "x86_64" || ([ "$TARGETARCH" = "arm64" ] && echo "aarch64" || echo "armv7")) && \
    curl -L -o /usr/local/bin/cargo-chef \
    https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.67/cargo-chef-v0.1.67-${ARCH}-unknown-linux-gnu && \
    chmod +x /usr/local/bin/cargo-chef

WORKDIR /service/hc-homie5-automation/

# Copy Cargo files + recipe from previous stage
COPY --from=chef /service/hc-homie5-automation/recipe.json recipe.json
COPY Cargo.toml Cargo.lock ./

# Cache dependency compilation (this layer will persist between builds if cache works correctly)
RUN cargo-chef cook --release --recipe-path recipe.json

# Final Build Stage - Application Source + Final Build
FROM rust:1.85-bookworm AS builder

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /service/hc-homie5-automation/

# Copy cached dependencies from builder-deps stage
COPY --from=builder-deps /usr/local/cargo /usr/local/cargo
COPY --from=builder-deps /service/hc-homie5-automation/target target

# Copy actual source code (now that deps are cached)
COPY . .

# Build Rust application
RUN cargo build --release && \
    strip target/release/hc-homie5-automation  # Optional but reduces image size

# Final Image - Minimal Runtime
FROM debian:bookworm-slim AS runtime

# Install only runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /service

# Copy the compiled binary from the builder stage
COPY --from=builder /service/hc-homie5-automation/target/release/hc-homie5-automation /service/

# Prepare required directories
RUN mkdir -p /service/rules /service/virtual_devices && \
    chown -R appuser:appuser /service

# Set environment variables (unchanged from your original)
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

