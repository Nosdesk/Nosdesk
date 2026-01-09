# Multi-stage build with optimized dependency caching
# Stage 1: Compute a recipe file for dependencies
FROM rust:1.87-slim as planner

# Install cargo-chef for dependency caching
RUN cargo install cargo-chef

WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build dependencies (this layer will be cached)
FROM rust:1.87-slim as cacher

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef and diesel CLI
RUN cargo install cargo-chef
RUN cargo install diesel_cli --no-default-features --features postgres

WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies - this layer will be cached unless dependencies change
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 3: Build the application
FROM rust:1.87-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the compiled dependencies from cacher stage
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

# Copy source code
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src
COPY backend/migrations ./migrations

# Build only the application (dependencies are already compiled)
RUN cargo build --release --bin backend

# Stage 4: Build frontend
FROM node:18-alpine as frontend-builder

WORKDIR /frontend

# Copy frontend package files
COPY frontend/package*.json ./

# Install ALL dependencies (including dev dependencies for build)
RUN npm ci

# Copy frontend source
COPY frontend .

# Build frontend for production (skip type checking for faster builds)
RUN npm run build-only

# Stage 5: Runtime image
FROM debian:bookworm-slim as runtime

# Install only runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    curl \
    wget \
    && rm -rf /var/lib/apt/lists/*

# Install pdfium library for PDF thumbnail generation
# Using pdfium-binaries from bblanchon's GitHub releases (widely used, regularly updated)
# https://github.com/bblanchon/pdfium-binaries
RUN mkdir -p /tmp/pdfium && \
    cd /tmp/pdfium && \
    wget -q https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz && \
    tar -xzf pdfium-linux-x64.tgz && \
    cp lib/libpdfium.so /usr/local/lib/ && \
    ldconfig && \
    rm -rf /tmp/pdfium

# Create non-root user for security
RUN useradd -m -u 1001 appuser

# Set working directory
WORKDIR /app

# Copy the compiled binary from builder stage
COPY --from=builder /app/target/release/backend ./backend

# Copy diesel CLI from builder stage
COPY --from=builder /usr/local/cargo/bin/diesel /usr/local/bin/diesel

# Copy migrations for automatic migration on startup
COPY --from=builder /app/migrations ./migrations

# Copy built frontend assets
COPY --from=frontend-builder /frontend/dist ./public

# Create upload directories with proper permissions
RUN mkdir -p uploads/temp uploads/tickets uploads/users/avatars uploads/users/banners uploads/users/thumbs && \
    chown -R appuser:appuser /app && \
    chmod +x /app/backend

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/health || exit 1

# Run the application directly (backend handles database initialization)
CMD ["./backend"] 