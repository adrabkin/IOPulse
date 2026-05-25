# Multi-stage build: compile on Linux, create minimal runtime image
FROM rust:1.70-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy source
COPY . .

# Build release binary
RUN cargo build --release

# Runtime stage - minimal image
FROM debian:bookworm-slim

# Install only runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /build/target/release/iopulse /usr/local/bin/iopulse

# Create directory for test files
RUN mkdir -p /data

WORKDIR /data

# Set the binary as entrypoint
ENTRYPOINT ["/usr/local/bin/iopulse"]

# Default help command
CMD ["--help"]
