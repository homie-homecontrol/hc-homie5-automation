# Build Stage
FROM rust:1.82-bullseye AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /service/hc-homie5-automation/

# Copy source code
COPY . .

# Build Rust application
RUN cargo build --release && \
    strip target/release/hc-homie5-automation  # Strip debug symbols

# Final Image
FROM debian:bullseye-slim AS runtime

ENV DEBIAN_FRONTEND=noninteractive

# Install libc runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends libc-bin && rm -rf /var/lib/apt/lists/*

# Install only runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

# Set working directory
WORKDIR /service

# Copy compiled binary
COPY --from=builder /service/hc-homie5-automation/target/release/hc-homie5-automation /service/

# Create required directories in a single step
RUN mkdir -p /service/rules /service/virtual_devices && \
    chown -R appuser:appuser /service

# Set environment variables
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

# Ensure proper execution of binary
ENTRYPOINT ["/service/hc-homie5-automation"]

