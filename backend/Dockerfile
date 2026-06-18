# syntax=docker/dockerfile:1.7
# Multi-stage build with cargo-chef dependency caching.
#
# The Rust stages share a PREBUILT cargo-chef base image (cargo-chef already
# installed), so nothing compiles cargo-chef from source. cargo-chef splits the
# dependency build into its own layer, cached unless Cargo.toml/lock change, so
# a source-only edit skips the heavy dep compile.

# Rust build profile. `release` (default) for published images; the dev-image
# workflow passes `fast` (opt-level 0, output dir target/fast) for quick staging
# builds. Declared before the first stage so each stage can re-`ARG` it; the
# `release` path runs the identical `--release` command as before.
ARG CARGO_PROFILE=release

# Prebuilt cargo-chef on rust 1.95 slim-bookworm (matches the prior base image).
FROM lukemathwalker/cargo-chef:0.1.77-rust-1.95.0-slim-bookworm AS chef
WORKDIR /app

# Stage 1: compute the dependency recipe.
FROM chef AS planner
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: build dependencies (this layer caches unless deps change).
FROM chef AS cacher
ARG CARGO_PROFILE
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
# `--release` (default) is the exact command used before; `fast` and any other
# named profile go through `--profile`.
RUN if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo chef cook --release --recipe-path recipe.json; \
    else \
      cargo chef cook --profile "$CARGO_PROFILE" --recipe-path recipe.json; \
    fi

# Stage 2b: diesel CLI, shipped for manual operator migrations
# (`docker compose exec backend diesel ...`). Built postgres-only to match the
# runtime's libpq. Its own stage so the compile runs in PARALLEL with the
# dependency cook above instead of blocking it.
FROM chef AS tools
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install diesel_cli --no-default-features --features postgres --locked

# Stage 3: build the application.
FROM chef AS builder
ARG CARGO_PROFILE
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Compiled dependencies + cargo caches from the cacher stage.
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo

# Copy source code
COPY backend/Cargo.toml backend/Cargo.lock ./
# build.rs hashes migrations → NOSDESK_SCHEMA_HASH (compile-time env for db/bootstrap/backup).
COPY backend/build.rs ./build.rs
COPY backend/src ./src
# license.rs embeds the Enterprise license verification key via
# include_str!("../license_pubkey.pem") — must exist at compile time.
COPY backend/license_pubkey.pem ./license_pubkey.pem
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

# Release version baked into the binary (e.g. 0.1.0-rc.5) for the admin
# System Information card, read via option_env!("NOSDESK_VERSION"). Empty in
# local/dev builds, where it falls back to the crate version. CI passes the
# git tag's version. Its value is part of the RUN command below, so changing
# it busts the build-cache layer (no stale version baked in).
ARG NOSDESK_VERSION=""

# Build the server + the nosdesk-cli admin tool together; both
# link against the same library crate and the extra binary is a
# trivial addition to a build we're doing anyway. The pubkey is
# passed inline rather than via `ENV` so it doesn't persist into
# the layer's metadata; it IS a public value, but keeping build
# secrets discipline tidy by convention.
RUN NOSDESK_ROOT_PUBKEY="$NOSDESK_ROOT_PUBKEY" \
    NOSDESK_VERSION="$NOSDESK_VERSION" \
    sh -c 'if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin backend --bin nosdesk-cli; \
    else \
      cargo build --profile "$CARGO_PROFILE" --bin backend --bin nosdesk-cli; \
    fi'

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
ARG CARGO_PROFILE

# Install only runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    curl \
    wget \
    && rm -rf /var/lib/apt/lists/*

# pdfium for PDF thumbnails. https://github.com/bblanchon/pdfium-binaries
# Pinned to a specific release and sha256-verified: `latest` plus no
# checksum meant a compromised or republished upstream release would
# land silently in the image, and builds were non-reproducible. To bump:
# choose a new tag, then update PDFIUM_VERSION and both checksums from
# that release's asset digests. See security-audit-2026-06.
ARG TARGETARCH
ARG PDFIUM_VERSION=chromium/7881
RUN case "$TARGETARCH" in \
      arm64) PDFIUM_PKG=pdfium-linux-arm64.tgz; PDFIUM_SHA256=ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674 ;; \
      amd64) PDFIUM_PKG=pdfium-linux-x64.tgz;   PDFIUM_SHA256=1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d ;; \
      *) echo "Unsupported TARGETARCH: $TARGETARCH" >&2 && exit 1 ;; \
    esac && \
    mkdir -p /tmp/pdfium && \
    cd /tmp/pdfium && \
    wget -q "https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_VERSION}/$PDFIUM_PKG" && \
    echo "${PDFIUM_SHA256}  ${PDFIUM_PKG}" | sha256sum -c - && \
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
# nosdesk-cli ...` without caring about its location. The profile dir
# matches CARGO_PROFILE (release -> target/release, fast -> target/fast).
COPY --from=builder /app/target/${CARGO_PROFILE}/backend ./backend
COPY --from=builder /app/target/${CARGO_PROFILE}/nosdesk-cli /usr/local/bin/nosdesk-cli

# Copy diesel CLI from the tools stage
COPY --from=tools /usr/local/cargo/bin/diesel /usr/local/bin/diesel

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
