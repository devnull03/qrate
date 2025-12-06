# Multi-stage Dockerfile for building qrate Tauri application
# Produces Linux binaries (.deb, .AppImage, etc.)

# =============================================================================
# Stage 1: Build environment
# =============================================================================
FROM rust:1.79-bookworm AS builder

# Install system dependencies for Tauri (Linux/GTK)
RUN apt-get update && apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    libssl-dev \
    libxdo-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    curl \
    wget \
    file \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js (LTS) and pnpm
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && npm install -g pnpm@latest

# Set working directory
WORKDIR /app

# Copy package files first for better layer caching
COPY package.json pnpm-lock.yaml ./

# Install frontend dependencies
RUN pnpm install --frozen-lockfile

# Copy Cargo files for Rust dependency caching
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/

# Create dummy main.rs and lib.rs to build dependencies
RUN mkdir -p src-tauri/src \
    && echo "fn main() {}" > src-tauri/src/main.rs \
    && echo "pub fn run() {}" > src-tauri/src/lib.rs

# Build Rust dependencies (cached layer)
WORKDIR /app/src-tauri
RUN cargo build --release 2>/dev/null || true
WORKDIR /app

# Copy the rest of the source code
COPY . .

# Rebuild with actual source
RUN cd src-tauri && cargo clean -p qrate

# Build the Tauri application
RUN pnpm tauri build

# =============================================================================
# Stage 2: Extract artifacts (minimal image with just the binaries)
# =============================================================================
FROM debian:bookworm-slim AS artifacts

# Install runtime dependencies (for running the app, if needed for testing)
RUN apt-get update && apt-get install -y \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libayatana-appindicator3-1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /artifacts

# Copy built artifacts from builder stage
COPY --from=builder /app/src-tauri/target/release/bundle/deb/*.deb ./deb/
COPY --from=builder /app/src-tauri/target/release/bundle/appimage/*.AppImage ./appimage/
COPY --from=builder /app/src-tauri/target/release/qrate ./bin/

# Default command: list artifacts
CMD ["ls", "-la", "/artifacts"]

# =============================================================================
# Usage:
# =============================================================================
# Build the image:
#   docker build -t qrate-builder .
#
# Extract artifacts to host:
#   docker create --name qrate-artifacts qrate-builder
#   docker cp qrate-artifacts:/artifacts ./dist
#   docker rm qrate-artifacts
#
# Or run interactively:
#   docker run --rm -it qrate-builder bash
#
# For CI/CD, you can also use BuildKit to output directly:
#   DOCKER_BUILDKIT=1 docker build --target artifacts --output type=local,dest=./dist .
# =============================================================================
