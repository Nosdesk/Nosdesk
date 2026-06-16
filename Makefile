# Nosdesk dev shortcuts. Run from the repo root.
#
# Prerequisites:
#   - OrbStack (recommended) or Docker Desktop, providing `docker` +
#     `docker compose`.
#   - GNU make. macOS ships with it; type `make --version` to confirm.
#
# `make dev` is the canonical command for daily work. Everything else
# is composed from it. See DEVELOPMENT.md for the full workflow doc.

COMPOSE := docker compose -f compose.yaml -f compose.dev.yaml

.DEFAULT_GOAL := help

.PHONY: help dev dev-bg dev-lan dev-bg-lan watch down clean clean-db restart restart-frontend migrate schema \
        test test-frontend logs logs-frontend shell psql mailpit token \
        install-hooks

help: ## Show this help message
	@echo "Available options:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# Foreground dev stack with `--watch` so source-file syncs are
# visible and reliable. Ctrl-C to stop (leaves volumes intact).
dev: ## Start the dev stack in the foreground (--watch; Ctrl-C to stop)
	$(COMPOSE) up --watch --build

# Same stack detached. Useful when you want `make logs` afterwards
# rather than a foreground tab. Source sync requires a separate
# `make watch` in another terminal.
dev-bg: ## Start the dev stack detached (pair with `make watch`)
	$(COMPOSE) up -d --build

# Same as `dev` / `dev-bg` but publishes the backend on 0.0.0.0 so
# other devices on your LAN can reach http://<this-machine-ip>:8080.
# Dev auth cookies are already non-Secure (ENVIRONMENT=development in
# compose.dev.yaml), so plain-HTTP access from a LAN IP works; the SPA,
# API, and collab WebSocket are all same-origin so nothing else needs
# changing. Only the backend is exposed: postgres / redis / mailpit
# stay bound to 127.0.0.1.
dev-lan: ## Start the dev stack exposed on the LAN (foreground)
	BACKEND_BIND=0.0.0.0 $(COMPOSE) up --watch --build

dev-bg-lan: ## Start the dev stack exposed on the LAN, detached (pair with `make watch`)
	BACKEND_BIND=0.0.0.0 $(COMPOSE) up -d --build

# Run only the file-sync watcher. Use after `make dev-bg` if you
# need source edits to reach the containers without restarting them.
watch: ## Run the file-sync watcher (use after `make dev-bg`)
	$(COMPOSE) watch

# Stop the stack, keep volumes (DB rows, cargo cache, node_modules
# stay on disk; next `make dev` is fast).
down: ## Stop the stack, keep volumes
	$(COMPOSE) down

# Stop the stack AND wipe volumes. Costs a 3-5 minute cold rebuild
# on the next `make dev` because cargo's target cache lives in a
# volume. Reserve for when you actively want a fresh DB.
clean: ## Stop the stack and wipe ALL volumes (triggers a cold rebuild)
	@echo "About to remove all volumes (postgres data, cargo cache, node_modules)."
	@echo "Cancel with Ctrl-C in the next 5 seconds, or wait to proceed."
	@sleep 5
	$(COMPOSE) down -v

# Wipe ONLY the postgres data volume. Keeps cargo cache + target dir,
# node_modules, and uploads intact, so the next `make dev` boots
# warm against a fresh database without paying the 3-5 minute cold
# build penalty `make clean` triggers. Use when iterating on
# migrations or onboarding flow.
clean-db: ## Wipe only the postgres data volume (keeps caches)
	@echo "About to remove the postgres data volume. Cargo + node caches preserved."
	@echo "Cancel with Ctrl-C in the next 5 seconds, or wait to proceed."
	@sleep 5
	$(COMPOSE) down
	docker volume rm nosdesk_postgres_data
	$(COMPOSE) up -d

# Restart just the backend container. Common when bacon stops
# rebuilding cleanly (it happens) or when you change something
# outside the source watch path (env vars, cargo config).
restart: ## Restart the nosdesk container
	$(COMPOSE) restart nosdesk

# Same for the frontend-watch service. Use when vite stops
# rebuilding (rarer but seen).
restart-frontend: ## Restart the frontend-watch container
	$(COMPOSE) restart frontend-watch

# Run pending diesel migrations against the dev DB. Schema regen
# happens via `make schema` (separate step because it touches a
# tracked file). The CLI runs as the privileged MIGRATION_DATABASE_URL
# role since the dev backend now connects as the unprivileged
# nosdesk_app; falls back to DATABASE_URL where the split isn't set.
migrate: ## Run pending diesel migrations
	$(COMPOSE) exec nosdesk sh -c 'DATABASE_URL="$${MIGRATION_DATABASE_URL:-$$DATABASE_URL}" diesel migration run'

# Regenerate backend/src/schema.rs from the live DB. Run after
# every successful `make migrate` if the migration touched table
# structure. RUST_LOG=off prevents stdout pollution corrupting the
# file. Uses the privileged role for full introspection.
schema: ## Regenerate backend/src/schema.rs from the live DB
	$(COMPOSE) exec nosdesk sh -c 'DATABASE_URL="$${MIGRATION_DATABASE_URL:-$$DATABASE_URL}" RUST_LOG=off diesel print-schema 2>/dev/null' > backend/src/schema.rs
	@echo "Regenerated backend/src/schema.rs"

# (Re)provision the nosdesk_app LOGIN role on an EXISTING dev DB, for the
# dev/prod parity flip without a full `make clean`. init-db-dev.sh does this
# automatically on a fresh data dir; this target covers a DB created before
# the flip. Idempotent.
dev-app-role: ## Grant the nosdesk_app runtime role LOGIN on an existing dev DB
	$(COMPOSE) exec postgres sh -c 'psql -v ON_ERROR_STOP=1 -U "$$POSTGRES_USER" -d "$$POSTGRES_DB" -c "ALTER ROLE nosdesk_app LOGIN PASSWORD '"'"'nosdesk_app_dev'"'"';"'
	@echo "nosdesk_app is now LOGIN-capable; restart the backend (make restart) to pick up the role."

# Backend unit + integration tests. Runs inside the container so
# libpq is available without a host-side install. `-j 1` keeps the
# memory footprint within the container's allowance.
test: ## Run backend tests (inside the container)
	$(COMPOSE) exec -e TEST_REDIS_URL=redis://:nosdesk_redis_password@redis:6379/15 nosdesk cargo test --tests -j 1 --no-fail-fast

# Frontend type-check (vue-tsc). Runs natively, not in a container,
# because tsc is the only frontend check that benefits from local
# tool versions.
test-frontend: ## Run the frontend type-check (vue-tsc)
	cd frontend && npm run type-check

# Follow nosdesk logs in the current terminal.
logs: ## Follow nosdesk logs
	$(COMPOSE) logs -f nosdesk

# Follow frontend-watch logs (vite build output).
logs-frontend: ## Follow frontend-watch logs (vite output)
	$(COMPOSE) logs -f frontend-watch

# Shell into the nosdesk container. Useful for ad-hoc diesel-cli
# invocations or environment inspection.
shell: ## Shell into the nosdesk container
	$(COMPOSE) exec nosdesk sh

# psql against the dev DB. The compose stack publishes postgres on
# 127.0.0.1:54329, so this also works from your host's psql if you
# have it installed.
psql: ## Open psql against the dev DB
	$(COMPOSE) exec postgres psql -U nosdesk -d nosdesk

# Open the Mailpit web UI in your default browser. Captured
# outbound mail lives there during dev.
mailpit: ## Open the Mailpit web UI
	@open http://localhost:8025

# Print the bootstrap token used for the initial-admin onboarding.
# Logged at backend startup, but easy to lose in scrollback. The dev
# image ships a `nosdesk-cli` shim on PATH that compiles + runs the CLI
# from the watch-synced source (first call compiles, cached after),
# matching production's `docker compose exec nosdesk nosdesk-cli
# setup-token`.
token: ## Print the first-run setup token + onboarding URL
	@$(COMPOSE) exec nosdesk nosdesk-cli setup-token

# Wire up the in-repo git hooks (one-shot per clone). Points git
# at .githooks/ for hook lookups so `pre-commit` runs rustfmt +
# eslint --fix on staged files before they land in a commit. Avoids
# the "cargo fmt broke CI again" loop on the merge side.
install-hooks: ## Install the in-repo git hooks (one-shot per clone)
	@git config core.hooksPath .githooks
	@echo "Installed: git hooks now resolve from .githooks/"
	@echo "Bypass any hook with 'git commit --no-verify' (CI will still gate)."
