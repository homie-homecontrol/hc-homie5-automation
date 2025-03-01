# Stage 1: Base Image with Rust and Dependencies
FROM rust:1.85-bookworm AS chef

# Install necessary system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libc6-dev lua5.4 liblua5.4-dev pkg-config && \
    rm -rf /var/lib/apt/lists/*

# Install cargo-chef using cargo
RUN cargo install cargo-chef --locked

# Set the working directory
WORKDIR /service/hc-homie5-automation/

# Copy Cargo.toml and Cargo.lock to the container
COPY Cargo.toml Cargo.lock ./

# Generate the cargo-chef recipe
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build Dependencies Layer
FROM chef AS builder-deps

# Copy the generated recipe to the container
COPY --from=chef /service/hc-homie5-automation/recipe.json recipe.json

# Cook (build) the dependencies
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 3: Build Application
FROM chef AS builder

# Copy the cooked dependencies
COPY --from=builder-deps /usr/local/cargo /usr/local/cargo
COPY --from=builder-deps /service/hc-homie5-automation/target target

# Copy the entire source code
COPY . .

# Build the Rust application
RUN cargo build --release && \
    strip target/release/hc-homie5-automation  # Strip debug symbols

# Stage 4: Runtime Image
FROM debian:bookworm-slim AS runtime

# Install necessary runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    lua5.4 liblua5.4-0 libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd --no-create-home --shell /usr/sbin/nologin appuser

# Set the working directory
WORKDIR /service

# Copy the compiled binary from the builder stage
COPY --from=builder /service/hc-homie5-automation/target/release/hc-homie5-automation /service/

# Create necessary directories and set permissions
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

# Switch to the non-root user
USER appuser

# Set the entrypoint to the compiled binary
ENTRYPOINT ["/service/hc-homie5-automation"]

