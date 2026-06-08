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

.PHONY: dev dev-bg watch down clean clean-db restart restart-frontend migrate schema \
        test test-frontend logs logs-frontend shell psql mailpit token \
        install-hooks

# Foreground dev stack with `--watch` so source-file syncs are
# visible and reliable. Ctrl-C to stop (leaves volumes intact).
dev:
	$(COMPOSE) up --watch --build

# Same stack detached. Useful when you want `make logs` afterwards
# rather than a foreground tab. Source sync requires a separate
# `make watch` in another terminal.
dev-bg:
	$(COMPOSE) up -d --build

# Run only the file-sync watcher. Use after `make dev-bg` if you
# need source edits to reach the containers without restarting them.
watch:
	$(COMPOSE) watch

# Stop the stack, keep volumes (DB rows, cargo cache, node_modules
# stay on disk; next `make dev` is fast).
down:
	$(COMPOSE) down

# Stop the stack AND wipe volumes. Costs a 3-5 minute cold rebuild
# on the next `make dev` because cargo's target cache lives in a
# volume. Reserve for when you actively want a fresh DB.
clean:
	@echo "About to remove all volumes (postgres data, cargo cache, node_modules)."
	@echo "Cancel with Ctrl-C in the next 5 seconds, or wait to proceed."
	@sleep 5
	$(COMPOSE) down -v

# Wipe ONLY the postgres data volume. Keeps cargo cache + target dir,
# node_modules, and uploads intact, so the next `make dev` boots
# warm against a fresh database without paying the 3-5 minute cold
# build penalty `make clean` triggers. Use when iterating on
# migrations or onboarding flow.
clean-db:
	@echo "About to remove the postgres data volume. Cargo + node caches preserved."
	@echo "Cancel with Ctrl-C in the next 5 seconds, or wait to proceed."
	@sleep 5
	$(COMPOSE) down
	docker volume rm nosdesk_postgres_data
	$(COMPOSE) up -d

# Restart just the backend container. Common when bacon stops
# rebuilding cleanly (it happens) or when you change something
# outside the source watch path (env vars, cargo config).
restart:
	$(COMPOSE) restart backend

# Same for the frontend-watch service. Use when vite stops
# rebuilding (rarer but seen).
restart-frontend:
	$(COMPOSE) restart frontend-watch

# Run pending diesel migrations against the dev DB. Schema regen
# happens via `make schema` (separate step because it touches a
# tracked file).
migrate:
	$(COMPOSE) exec backend diesel migration run

# Regenerate backend/src/schema.rs from the live DB. Run after
# every successful `make migrate` if the migration touched table
# structure. RUST_LOG=off prevents stdout pollution corrupting the
# file.
schema:
	$(COMPOSE) exec backend sh -c 'RUST_LOG=off diesel print-schema 2>/dev/null' > backend/src/schema.rs
	@echo "Regenerated backend/src/schema.rs"

# Backend unit + integration tests. Runs inside the container so
# libpq is available without a host-side install. `-j 1` keeps the
# memory footprint within the container's allowance.
test:
	$(COMPOSE) exec backend cargo test --tests -j 1 --no-fail-fast

# Frontend type-check (vue-tsc). Runs natively, not in a container,
# because tsc is the only frontend check that benefits from local
# tool versions.
test-frontend:
	cd frontend && npm run type-check

# Follow backend logs in the current terminal.
logs:
	$(COMPOSE) logs -f backend

# Follow frontend-watch logs (vite build output).
logs-frontend:
	$(COMPOSE) logs -f frontend-watch

# Shell into the backend container. Useful for ad-hoc diesel-cli
# invocations or environment inspection.
shell:
	$(COMPOSE) exec backend sh

# psql against the dev DB. The compose stack publishes postgres on
# 127.0.0.1:54329, so this also works from your host's psql if you
# have it installed.
psql:
	$(COMPOSE) exec postgres psql -U nosdesk -d helpdesk

# Open the Mailpit web UI in your default browser. Captured
# outbound mail lives there during dev.
mailpit:
	@open http://localhost:8025

# Print the bootstrap token used for the initial-admin onboarding.
# Logged at backend startup, but easy to lose in scrollback.
token:
	@$(COMPOSE) exec backend nosdesk-cli setup-token

# Wire up the in-repo git hooks (one-shot per clone). Points git
# at .githooks/ for hook lookups so `pre-commit` runs rustfmt +
# eslint --fix on staged files before they land in a commit. Avoids
# the "cargo fmt broke CI again" loop on the merge side.
install-hooks:
	@git config core.hooksPath .githooks
	@echo "Installed: git hooks now resolve from .githooks/"
	@echo "Bypass any hook with 'git commit --no-verify' (CI will still gate)."
