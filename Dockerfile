# Stage 1: Base Image with Rust and Dependencies
FROM ghcr.io/homie-homecontrol/hc-homie5-automation/base:latest AS chef

# Set working directory (Cargo expects this)
WORKDIR /service/hc-homie5-automation/

# Copy manifests (only the dependency files - not code)
COPY Cargo.toml Cargo.lock ./

# Generate the cargo-chef recipe (dependency graph)
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build Dependency Cache using cargo chef and Buildx cache mounts
FROM chef AS builder-deps

# Same working directory (Cargo expects consistency)
WORKDIR /service/hc-homie5-automation/

# Bring in the recipe from the previous stage
COPY --from=chef /service/hc-homie5-automation/recipe.json recipe.json

# Run cargo chef cook, with proper cache mounts for registry and target dir.
# This is the "magic" - Buildx populates cache, cargo chef just works.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/service/hc-homie5-automation/target \
    cargo chef cook --release --recipe-path recipe.json

# Stage 3: Build Final Application with Caching
FROM chef AS builder

# Same working directory (Cargo expects consistency)
WORKDIR /service/hc-homie5-automation/

# Copy full source code now (the actual app)
COPY . .

# Inject version (only affects final build - does not invalidate deps cache)
ARG VERSION=0.0.0-placeholder
RUN sed -i "s/^version = \"0.0.0-placeholder\"/version = \"$VERSION\"/" Cargo.toml

# Final build - and reuse exact same cache mounts so we get incremental build!
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/service/hc-homie5-automation/target \
    cargo build --release && \
    strip target/release/hc-homie5-automation

# Stage 4: Minimal Runtime Image
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create runtime user
RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

# Set workdir for the final container
WORKDIR /service

# Bring in just the compiled binary from the build stage
COPY --from=builder /service/hc-homie5-automation/target/release/hc-homie5-automation /service/

# Prepare runtime folders and permissions
RUN mkdir -p /service/rules /service/virtual_devices && \
    chown -R appuser:appuser /service

# Environment variables (unchanged)
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

# Switch to non-root runtime user
USER appuser

# Set entrypoint to the final binary
ENTRYPOINT ["/service/hc-homie5-automation"]

