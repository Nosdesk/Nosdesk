<p align="center">
  <img src="logo.svg" alt="Nosdesk" width="400">
</p>

<p align="center">
  <strong>A modern helpdesk built for teams who value speed and simplicity</strong>
</p>

<p align="center">
  <a href="https://nosdesk.com">Website</a> ·
  <a href="https://nosdesk.com/docs">Documentation</a> ·
  <a href="https://github.com/kylephillipsau/Nosdesk/issues">Report a Bug</a>
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
git clone https://github.com/kylephillipsau/Nosdesk.git
cd Nosdesk

# Create your environment file
cp docker.env.example docker.env
```

Open `docker.env` and set the required values:

```bash
# Generate these with: openssl rand -base64 32
JWT_SECRET=your-generated-secret

# Generate with: openssl rand -hex 32
MFA_ENCRYPTION_KEY=your-generated-key

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
# Start with hot reloading
docker compose -f compose.yaml -f compose.dev.yaml up -d --build

# View logs
docker compose -f compose.yaml -f compose.dev.yaml logs -f

# Run database migrations
docker compose -f compose.yaml -f compose.dev.yaml exec backend diesel migration run
```

For API testing, import `api-insomnia.json` into [Insomnia](https://insomnia.rest/).

## License

This project is licensed under the Business Source License. See the [LICENSE](LICENSE) file for details.
