# ===== Stage 1: Build the Rust server =====
FROM rust:bookworm AS builder

WORKDIR /app
COPY . .

RUN cargo build --release --bin server

# ===== Stage 2: Runtime with Python, Node, and G++ =====
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 \
    nodejs \
    g++ \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create symlink so "python" command works
RUN ln -sf /usr/bin/python3 /usr/bin/python

WORKDIR /app

# Copy the compiled binary from builder
COPY --from=builder /app/target/release/server /app/server_bin

# Copy static files
COPY server/static /app/server/static

# Expose port
EXPOSE 8080

# Run the server
CMD ["./server_bin"]
