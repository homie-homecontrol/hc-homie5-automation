FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

WORKDIR /service

# Copy pre-built binary directly from GitHub workflow into image
COPY hc-homie5-automation /service/

# Create required directories
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

# Drop privileges
USER appuser

ENTRYPOINT ["/service/hc-homie5-automation"]

