<p align="center">
  <img src="logo.svg" alt="Nosdesk" width="400">
</p>

<p align="center">
  <strong>A modern helpdesk built for teams who value speed and simplicity</strong>
</p>

<p align="center">
  <a href="https://nosdesk.com">Website</a> ·
  <a href="https://nosdesk.com/docs">Documentation</a> ·
  <a href="https://github.com/Nosdesk/Nosdesk/issues">Report a Bug</a>
</p>

---

> [!CAUTION]
> Nosdesk is under active development and not yet production-ready. Expect breaking changes as the project evolves. Use at your own risk.

## What is Nosdesk?

Nosdesk is an open source helpdesk built for frictionless collaboration. Every part of the system, from tickets and projects to users, devices, and documentation, is designed to let teams work together without getting in the way.

## Features

- **Tickets** with real-time collaborative editing, voice notes, and file attachments
- **Projects** with Kanban boards and progress tracking
- **Documentation** built on real experience, seamlessly incorporating ticket notes into a readily accessible knowledge base
- **Users and Devices** linked to relevant tickets, with Microsoft Intune sync
- **Authentication** via local accounts with MFA, Microsoft Entra ID, or any OIDC provider
- **Theming** with dark mode and custom branding

## Quick Start

You'll need Docker and Docker Compose installed.

```bash
# Clone the repository
git clone https://github.com/Nosdesk/Nosdesk.git
cd Nosdesk

# Create your environment file
cp docker.env.example docker.env
```

Open `docker.env` and set the required values:

```bash
# Generate these with: openssl rand -base64 32
JWT_SECRET=your-generated-secret

# At-rest encryption KEK. Generate with: openssl rand -hex 32
# Versioned for zero-downtime rotation; see docs for MFA_KEK_VERSION.
MFA_KEK_V1=your-generated-key

# Change these from the defaults
POSTGRES_PASSWORD=choose-a-strong-password
REDIS_PASSWORD=choose-a-strong-password
```

Start the application:

```bash
docker compose up -d --build
```

Open [http://localhost:8080](http://localhost:8080) in your browser. On first launch, you'll be guided through creating your admin account.

## Technology

| Component | Stack |
|-----------|-------|
| Backend | Rust, Actix-web, PostgreSQL, Redis |
| Frontend | Vue.js 3, TypeScript, Tailwind CSS |
| Real-time | WebSockets, Yjs CRDT, ProseMirror |

## Development

```bash
# Start with hot reloading (binds to 127.0.0.1 only)
docker compose -f compose.yaml -f compose.dev.yaml up -d --build

# View logs
docker compose -f compose.yaml -f compose.dev.yaml logs -f

# Run database migrations
docker compose -f compose.yaml -f compose.dev.yaml exec backend diesel migration run
```

By default the dev backend is reachable only from `localhost` to avoid LAN
exposure. To test collaboration features from other devices, expose the
backend on all interfaces:

```bash
BACKEND_BIND=0.0.0.0 docker compose -f compose.yaml -f compose.dev.yaml up -d --build
```

For API testing, import `api-insomnia.json` into [Insomnia](https://insomnia.rest/).

## CLI tools

Nosdesk ships a `nosdesk-cli` binary for plugin authoring, signing, and a
handful of break-glass admin operations against a running instance.

### Install

If you have a local Rust toolchain:

```bash
cargo install --path backend --bin nosdesk-cli
```

This drops the binary at `~/.cargo/bin/nosdesk-cli`. Re-run the same
command after pulling Nosdesk updates; cargo's incremental build only
rebuilds what changed.

The `backend` crate links against `libpq` (PostgreSQL client). Install
it from your package manager before the first `cargo install`:

```bash
# macOS (libpq is keg-only on Homebrew)
brew install libpq
export LIBRARY_PATH="/opt/homebrew/opt/libpq/lib:$LIBRARY_PATH"
export PKG_CONFIG_PATH="/opt/homebrew/opt/libpq/lib/pkgconfig:$PKG_CONFIG_PATH"

# Debian/Ubuntu
sudo apt install libpq-dev pkg-config
```

Add the macOS exports to your shell rc so future installs pick them
up automatically.

If you don't want a local toolchain, the same binary ships in the
production Docker image at `/usr/local/bin/nosdesk-cli`:

```bash
docker compose run --rm --no-deps --entrypoint nosdesk-cli backend --help
```

### Subcommands

```
nosdesk-cli plugin gen-key  --out ~/.nosdesk/plugin-key
nosdesk-cli plugin sign     <plugin-dir> --key <sk> --out <zip> --source <tier>
nosdesk-cli plugin verify   <zip>
nosdesk-cli plugin install  <zip>

nosdesk-cli admin reset-password <email>
nosdesk-cli admin clear-mfa      <email>
```

`plugin sign` and friends are pure file-IO; they don't need a running
Nosdesk. `plugin install` and the `admin` subcommands talk to the
configured database directly, so they need `DATABASE_URL` set (the
backend's `docker.env` already exports it for the Docker workflow).

### Signing a plugin

```bash
cd ~/dev/<plugin-repo>
mkdir -p dist
nosdesk-cli plugin sign . \
  --key <path-to-your-signing-key> \
  --out dist/<plugin-name>-<version>.zip \
  --source <nosdesk-root|verified-publisher|community-publisher|local>
shasum -a 256 dist/<plugin-name>-<version>.zip
```

The `--source` value must match how your signing key is registered with
the trust chain on the target Nosdesk instance(s). See
`docs/plugin-registry-plan.md` (this repo) for how trust tiers resolve
on the install pipeline.

## License

Nosdesk is licensed under the Business Source License 1.1. See the [LICENSE](LICENSE) file for details.

Copyright (c) 2026 Nosdesk Pty Ltd.

## Trademark

Nosdesk&trade; and the Nosdesk logo are trademarks of Nosdesk Pty Ltd. The license above grants no right to use these marks (see the trademark reservation in the [LICENSE](LICENSE)); you may not use the Nosdesk name or logo to brand a derivative or fork in a way that implies endorsement or affiliation.
