<p align="center">
  <img src="logo.svg" alt="Nosdesk" width="400">
</p>

<p align="center">
  A modern helpdesk and IT management system built with Rust and Vue.js
</p>

<p align="center">
  <a href="https://nosdesk.com">Website</a> •
  <a href="https://nosdesk.com/docs">Documentation</a>
</p>

---

> [!CAUTION]
> This project is in active development, do not use it until it is stable and production ready. I am moving fast and breaking things!
> This software is provided as-is with absolutely no warranty. Use at your own risk!

## Features

- **Ticket Management** — Track issues with comments, attachments, linked tickets, and real-time collaborative notes
- **Project Organization** — Group related tickets into projects with progress tracking
- **User & Device Management** — Role-based access control with device assignment and tracking
- **Flexible Authentication** — Local accounts with TOTP MFA, Microsoft Entra ID, or any OIDC provider
- **Real-time Collaboration** — Live editing powered by WebSockets with ProseMirror and Yjs
- **Microsoft Integration** — Import users and devices from Intune and Entra ID

## Quick Start

**Prerequisites:** Docker and Docker Compose

```bash
# 1. Clone the repository
git clone https://github.com/kylephillipsau/Nosdesk.git
cd Nosdesk

# 2. Create environment configuration
cp docker.env.example docker.env

# 3. Update required environment variables in docker.env:
# - JWT_SECRET: Generate with `openssl rand -base64 32`
# - MFA_ENCRYPTION_KEY: Generate with `openssl rand -hex 32`
# - POSTGRES_PASSWORD: Change from default
# - REDIS_PASSWORD: Change from default

# 4. Start the application (production)
docker compose up -d --build
```

Access the app at [http://localhost:8080](http://localhost:8080)

**First-time setup:** The application will guide you through creating an admin account on first launch.

## Technology Stack

- **Backend**: Rust with Actix-web
- **Frontend**: Vue.js 3 with TypeScript and Tailwind CSS
- **Database**: PostgreSQL with Redis caching
- **Real-time**: WebSockets and Server-Sent Events
- **Authentication**: JWT with optional TOTP MFA
- **Integrations**: Microsoft Graph API for Intune/Entra ID

## Project Structure

```
Nosdesk/
├── backend/           # Rust API server
│   ├── src/
│   ├── migrations/    # Database schema
│   └── public/        # Built frontend (production)
├── frontend/          # Vue.js frontend
│   ├── src/
│   └── public/
└── compose.yaml       # Docker orchestration
```

## Configuration

All configuration is managed through `docker.env` which includes:

- **Security**: JWT_SECRET, MFA_ENCRYPTION_KEY (generate secure keys for production)
- **Database**: PostgreSQL connection settings (default: `helpdesk` database)
- **Redis**: Cache and session storage configuration
- **Microsoft Integration** (Optional): Entra ID SSO and Intune device management
- **OIDC** (Optional): Generic OpenID Connect provider configuration
- **SMTP**: Email notifications for tickets and alerts

For local development outside Docker, the backend will auto-generate `.env` from the example file.

## Documentation

- **API Documentation**: Import `api-insomnia.json` into Insomnia

## Contributing

### Development Environment

For active development with hot reloading:

```bash
# Start development environment
docker compose -f compose.yaml -f compose.dev.yaml up -d --build

# Access services:
# - Application: http://localhost:8080
# - Backend logs: docker compose -f compose.yaml -f compose.dev.yaml logs -f backend
# - Frontend logs: docker compose -f compose.yaml -f compose.dev.yaml logs -f frontend-watch
```

**Development stack includes:**
- **postgres**: PostgreSQL database with persistent storage
- **redis**: Redis for caching and real-time features
- **backend**: Rust API with hot reload and automatic migrations (dev mode)
- **frontend-watch**: Vue.js dev server with Hot Module Replacement (HMR)

**Database migrations:**
```bash
# Apply migrations
docker compose -f compose.yaml -f compose.dev.yaml exec backend diesel migration run

# Regenerate schema
docker compose -f compose.yaml -f compose.dev.yaml exec backend sh -c 'diesel print-schema > src/schema.rs'
```


## License

Licensed under the terms in the LICENSE file.
