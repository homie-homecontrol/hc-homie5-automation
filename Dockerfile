# Stage 1: Base Image with Rust and Dependencies
FROM ghcr.io/homie-homecontrol/hc-homie5-automation/base:latest AS chef

WORKDIR /service/hc-homie5-automation/

# Copy manifests (dependencies only, no code)
COPY Cargo.toml Cargo.lock ./

# Generate the dependency graph (recipe)
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build Dependency Cache using cargo chef and cache mounts
FROM chef AS builder-deps

WORKDIR /service/hc-homie5-automation/

# Copy the recipe from the previous stage
COPY --from=chef /service/hc-homie5-automation/recipe.json recipe.json

# Build dependencies with cache mounts for both registry and target
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/service/hc-homie5-automation/target \
    cargo chef cook --release --recipe-path recipe.json

# Stage 3: Build Final Application using the cached deps
FROM chef AS builder

WORKDIR /service/hc-homie5-automation/

# Copy full source code now
COPY . .

# Inject version
ARG VERSION=0.0.0-placeholder
RUN sed -i "s/^version = \"0.0.0-placeholder\"/version = \"$VERSION\"/" Cargo.toml

# Build final binary with cache mounts (same cache as cook step)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/service/hc-homie5-automation/target \
    cargo build --release && \
    strip target/release/hc-homie5-automation

# Stage 4: Minimal Runtime Image (Responsible for Final Copy)
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 libssl3 && \
    rm -rf /var/lib/apt/lists/*

RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /service

# mount the same cache and extracts the binary
RUN --mount=type=cache,target=/service/hc-homie5-automation/target \
    cp /service/hc-homie5-automation/target/release/hc-homie5-automation /service/

# Prepare runtime folders and permissions
RUN mkdir -p /service/rules /service/virtual_devices && \
    chown -R appuser:appuser /service

# Environment (no changes here)
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

USER appuser
ENTRYPOINT ["/service/hc-homie5-automation"]

