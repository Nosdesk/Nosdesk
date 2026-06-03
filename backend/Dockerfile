# syntax=docker/dockerfile:1.7
# Multi-stage build with optimized dependency caching
# Stage 1: Compute a recipe file for dependencies
FROM rust:1.95-slim-bookworm AS planner

# Install cargo-chef for dependency caching
RUN cargo install cargo-chef --locked

WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: Build dependencies (this layer will be cached)
FROM rust:1.95-slim-bookworm AS cacher

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef and diesel CLI
RUN cargo install cargo-chef --locked
RUN cargo install diesel_cli --no-default-features --features postgres --locked

WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies - this layer will be cached unless dependencies change
RUN cargo chef cook --release --recipe-path recipe.json

# Stage 3: Build the application
FROM rust:1.95-slim-bookworm AS builder

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
# build.rs hashes migrations → NOSDESK_SCHEMA_HASH (compile-time env for db/bootstrap/backup).
COPY backend/build.rs ./build.rs
COPY backend/src ./src
COPY backend/migrations ./migrations
# seed.rs uses include_str!("../../seeds/...") — must exist at compile time.
COPY backend/seeds ./seeds
# starter_catalog.rs uses include_str!("../../data/starter-rules.json")
# — same compile-time requirement.
COPY backend/data ./data
# Shared Fluent catalogues. include_str! in src/utils/i18n.rs
# resolves them via ../../../i18n/locales/... so mirror the host
# workspace layout at /i18n inside the container.
COPY i18n /i18n

# Ed25519 root pubkey for the plugin registry trust chain. Baked
# into the compiled binary via `option_env!("NOSDESK_ROOT_PUBKEY")`
# in services/plugins/signing.rs. Published at
# https://nosdesk.com/registry/root.pub for anyone reproducing an
# official Nosdesk build. Forks that want their own trust root
# override this at build time:
#
#     docker build --build-arg NOSDESK_ROOT_PUBKEY='<base64>' ...
#
# Empty default: a binary built without the arg has `root_pubkey()
# == None`, official + verified installs fail closed, only the
# CLI-installed `local` tier and filesystem-provisioned plugins
# work. Correct for an unconfigured fork, useless for an official
# release — CI must set the arg.
ARG NOSDESK_ROOT_PUBKEY=""

# Build the server + the nosdesk-cli admin tool together; both
# link against the same library crate and the extra binary is a
# trivial addition to a build we're doing anyway. The pubkey is
# passed inline rather than via `ENV` so it doesn't persist into
# the layer's metadata; it IS a public value, but keeping build
# secrets discipline tidy by convention.
RUN NOSDESK_ROOT_PUBKEY="$NOSDESK_ROOT_PUBKEY" \
    cargo build --release --bin backend --bin nosdesk-cli

# Stage 4: Build frontend
FROM node:24-bookworm-slim AS frontend-builder

WORKDIR /frontend

# Copy frontend package files
COPY frontend/package*.json ./

# Install ALL dependencies (including dev dependencies for build)
RUN npm ci

# Copy frontend source
COPY frontend .
# Shared Fluent catalogues. Vite's `import.meta.glob` in
# src/i18n/index.ts resolves `../../../i18n/locales/...` to `/i18n`
# from inside `/frontend/src/i18n/`, so mirror the workspace
# layout there.
COPY i18n /i18n

# Build_sha stamped into the bundle so bug reports can identify
# which deployment a client was running. Passed in by the operator's
# build pipeline via `--build-arg VITE_BUILD_SHA=$(git rev-parse --short HEAD)`.
# Defaults to "unknown" if unset so the build never fails on a
# missing arg. Placed after `npm ci` and the source COPY so
# changing the SHA does not invalidate the dependency layer cache.
ARG VITE_BUILD_SHA=unknown
ENV VITE_BUILD_SHA=$VITE_BUILD_SHA

# Build frontend for production (skip type checking for faster builds)
RUN npm run build-only

# Stage 5: Runtime image
FROM debian:bookworm-slim AS runtime

# Install only runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    curl \
    wget \
    && rm -rf /var/lib/apt/lists/*

# pdfium for PDF thumbnails. https://github.com/bblanchon/pdfium-binaries
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      arm64) PDFIUM_PKG=pdfium-linux-arm64.tgz ;; \
      amd64) PDFIUM_PKG=pdfium-linux-x64.tgz ;; \
      *) echo "Unsupported TARGETARCH: $TARGETARCH" >&2 && exit 1 ;; \
    esac && \
    mkdir -p /tmp/pdfium && \
    cd /tmp/pdfium && \
    wget -q "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/$PDFIUM_PKG" && \
    tar -xzf "$PDFIUM_PKG" && \
    cp lib/libpdfium.so /usr/local/lib/ && \
    ldconfig && \
    rm -rf /tmp/pdfium

# Create non-root user for security
RUN useradd -m -u 1001 appuser

# Set working directory
WORKDIR /app

# Copy the compiled binaries from builder stage. `nosdesk-cli` is
# placed on PATH so admins can `docker compose exec backend
# nosdesk-cli ...` without caring about its location.
COPY --from=builder /app/target/release/backend ./backend
COPY --from=builder /app/target/release/nosdesk-cli /usr/local/bin/nosdesk-cli

# Copy diesel CLI from builder stage
COPY --from=builder /usr/local/cargo/bin/diesel /usr/local/bin/diesel

# Copy migrations for automatic migration on startup
COPY --from=builder /app/migrations ./migrations

# Copy seed data files
COPY backend/seeds ./seeds

# Copy built frontend assets
COPY --from=frontend-builder /frontend/dist ./public

# Create upload directories with proper permissions. /app/state holds
# operator state that must never sit under the user-content upload tree
# (currently the bootstrap.token; see utils::bootstrap_token).
RUN mkdir -p uploads/temp uploads/tickets uploads/users/avatars uploads/users/banners uploads/users/thumbs state && \
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