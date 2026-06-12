# Development setup

Daily-workflow doc for working on Nosdesk. If you've cloned the repo
and want to get to "I can edit code and see it reload" in under 10
minutes, follow §1 and §2 below.

For architecture and feature explanations, see `CLAUDE.md`. For roadmap
items and design plans, see `docs/`.

---

## 1. Prerequisites

### Container runtime

**Recommended: [OrbStack](https://orbstack.dev/).** It's a Docker
Desktop replacement built for macOS: faster boot, more reliable file
sync via virtio-fs, lower memory overhead. Drop-in for the `docker`
and `docker compose` CLIs.

```bash
brew install --cask orbstack
```

On first launch, OrbStack offers to migrate containers and volumes
from Docker Desktop if you have it installed. Accept the migration
to keep your cargo cache and any existing dev DB.

**Alternative**: Docker Desktop. Works, but file watching is less
reliable on macOS and rebuilds are slower. If you stick with it,
allocate at least 10 GB of memory in Docker Desktop's settings (the
backend test crate OOMs below that when bacon builds concurrently).

### Host tools

- **GNU make**: ships with macOS. `make --version` to confirm.
- **Node 20+**: for `make test-frontend` and any ad-hoc frontend
  scripting. The dev stack runs Vite inside a container so a host-side
  install is optional, but it's convenient.

No host-side Rust or Postgres install required. Everything runs in
containers.

---

## 2. First-time setup

From the repo root:

```bash
cp .env.example .env  # if you haven't already
make install-hooks                # one-shot per clone
make dev
```

`make install-hooks` points git at `.githooks/`, so `pre-commit`
runs rustfmt + eslint --fix on staged files before they land in a
commit. Without this, CI's `cargo fmt --check` and ESLint steps
will reject mis-formatted code at the merge gate. Bypass with
`git commit --no-verify` (CI still gates).

`make dev` runs `docker compose -f compose.yaml -f compose.dev.yaml up
--watch --build`. First boot does a full cargo build (~5 minutes) and
a full vite build (~30 seconds). Subsequent boots reuse the build
caches and start in under 30 seconds.

While the foreground stack runs, open a second terminal and apply
migrations:

```bash
make migrate
```

Then visit http://localhost:8080. The backend's startup logs (visible
in your `make dev` terminal) include a one-time bootstrap URL of the
form `http://localhost:8080/onboarding?token=<random>`. Click that
URL, fill in the admin form, and you're in. The token expires after
60 minutes; if it lapses, `make restart` rotates it.

If you lose the token in scrollback, `make token` prints the current
one.

---

## 3. Daily workflow

Most days, two commands:

```bash
make dev      # foreground, watch live
# ... edit code ...
# ... save ...
# ... bacon rebuilds backend, vite rebuilds frontend ...
# ... browser-reloaded automatically ...

# Ctrl-C when done. Volumes stay; tomorrow's `make dev` is fast.
```

If you prefer a single terminal:

```bash
make dev-bg                   # stack detached
make watch &                  # source sync in background
make logs                     # follow backend output
```

### Serving on the LAN

`make dev-lan` / `make dev-bg-lan` mirror `make dev` / `make dev-bg` but
publish the backend on `0.0.0.0` so other devices can reach it at
`http://<lan-ip>:8080` (`ipconfig getifaddr en0`). Only the backend is
exposed; everything else stays on `127.0.0.1`. Trusted networks only.

### Common operations

| Task | Command |
|---|---|
| Apply pending migrations | `make migrate` |
| Regenerate `schema.rs` after a migration | `make schema` |
| Run backend tests | `make test` |
| Run frontend type-check | `make test-frontend` |
| Tail backend logs | `make logs` |
| Tail frontend (vite) logs | `make logs-frontend` |
| Shell into backend container | `make shell` |
| Open the dev Postgres in psql | `make psql` |
| Open the captured-mail Mailpit UI | `make mailpit` |
| Print the bootstrap token (if onboarding incomplete) | `make token` |

For anything not in the Makefile, you can always fall back to the raw
compose command. The Makefile is just `docker compose -f compose.yaml
-f compose.dev.yaml` with shorter names.

### Stopping and starting

| Command | Effect |
|---|---|
| `Ctrl-C` in `make dev` foreground | Stop containers, keep volumes |
| `make down` | Stop containers, keep volumes (same as Ctrl-C in dev-bg case) |
| `make clean` | Stop containers, wipe ALL volumes (DB, cargo cache, node_modules). 5-minute rebuild on the next `make dev`. |
| `make restart` | Restart just the backend container. Useful when bacon stops rebuilding cleanly. |
| `make restart-frontend` | Restart just the frontend-watch container. |

---

## 4. Gotchas

These will bite you at some point. Save yourself the debugging.

### 4.1 `--watch` must be running for source edits to reach the container

The dev compose stack uses `develop.watch` (Compose 2.22+) to sync
host source-file edits into the container's filesystem. Without the
watch process running, you can edit `backend/src/main.rs` all you
want and nothing changes in the running backend.

`make dev` runs `up --watch` in one command so this is automatic.
If you use `make dev-bg` you MUST also run `make watch` (or `docker
compose ... watch` manually) for the sync to happen.

**Symptom**: you edited a file, saved, and `cargo build` inside the
container doesn't rebuild. Or the response doesn't change. Or worse,
bacon claims the file changed but recompiles the OLD source.

**Fix**: confirm watch is running. If unsure, `make restart` forces
bacon to recompile from whatever source it currently has on disk.

### 4.2 `make clean` wipes the cargo cache

`make clean` runs `docker compose down -v`, which removes ALL the dev
stack's named volumes, including `backend_cargo_cache` and
`backend_target_cache`. The next `make dev` does a full cold cargo
build, taking 3-5 minutes.

Only run `make clean` when you actually want a fresh DB (after
migration testing, after destructive seed scripts, etc.). For routine
"my container is in a weird state" cleanup, prefer `make down`
followed by `make dev`.

### 4.3 Bootstrap token expires after 60 minutes

The first-boot admin-onboarding URL carries a token that's good for
60 minutes from backend start. If you leave a fresh dev DB sitting
around for an hour without completing onboarding, the URL stops
working with a "token expired" error.

**Fix**: `make restart`. Reconcile mints a fresh token at startup
when no users exist yet; check the new bootstrap URL in the logs or
run `make token`.

### 4.4 Bacon-not-rebuilding after large changes

Rare, but documented in commit history. Symptom: bacon says "watching
for changes" but doesn't fire on edits. Inside the container,
`touch /app/src/main.rs` doesn't help.

**Fix**: `make restart`. The container restarts bacon from scratch
and it picks up the current source state.

### 4.5 Vite-watch silently stalls

Even rarer. The `frontend-watch` container's `vite build --watch`
process sometimes stops writing to `/app/dist`. Frontend changes
appear to not load.

**Fix**: `make restart-frontend`.

### 4.6 Frontend and backend on the same port

Both the production-mode backend (`docker compose up`) AND the dev
stack serve everything on port 8080. The backend serves the
frontend's built output from `backend/public/` (populated by the
frontend-watch container). There's NO separate Vite dev server in
this setup; HMR doesn't apply, you get full-page reloads on
frontend changes.

If you ever switch to a "Vite dev server on 5173 + backend on 8080"
shape, you'll need a Vite proxy block for `/api/*`. Not configured
today.

### 4.7 Real SMTP testing requires switching compose files

The dev stack overrides `SMTP_HOST` to point at Mailpit. Outbound
mail lands at http://localhost:8025 (the Mailpit UI). To exercise
real-SMTP delivery, stop the dev stack and run:

```bash
docker compose up -d --build  # production-mode, no `-f compose.dev.yaml`
```

This honors the `SMTP_*` settings in `.env`. Useful for
debugging "does Mailgun accept our message-id format?" kinds of
issues.

---

## 5. Troubleshooting checklist

When something is wrong, work through this list before posting in
chat:

1. **Is the container running?** `docker compose -f compose.yaml -f
   compose.dev.yaml ps`. If any service is `unhealthy` or `exited`,
   that's your culprit.

2. **Are recent edits reflected in the container?** `make shell`
   then `grep <something-recent> /app/src/main.rs`. If your edit
   isn't there, your watch isn't running.

3. **Is bacon recompiling?** `make logs` and look for `Compiling
   backend v0.1.0`. If you see only scheduler-tick logs and no
   compile lines for the last 30 seconds, bacon is idle (maybe
   stuck). `make restart`.

4. **Did the DB migrate?** `make migrate` is idempotent; safe to
   re-run. If a migration is partially applied (very rare), `make
   clean && make dev && make migrate` resets.

5. **Tests pass?** `make test`. The pre-existing
   `services::backup::tests::restore_table_data_ignores_hostile_column_names`
   failure is known and tracked; everything else should pass.

6. **Frontend type-checks?** `make test-frontend`. If this fails
   after pulling, it usually means a new Vue/TS dep needs `npm
   install` inside `frontend/`.

---

## 6. Working with worktrees

The repo uses `.claude/worktrees/` for branch-isolated work. If you
see paths like `.claude/worktrees/ticket-merge/...` in commits or
docs, that's a sibling working tree for that feature branch. To
list them:

```bash
git worktree list
```

Worktrees share the `.git` directory but have separate working
trees. Editing in `.claude/worktrees/X/` affects the `worktree-X`
branch only.

---

## 7. Data export vs database backup

**CSV export** (Assets list → Export CSV, or `GET /api/assets/export`)
is for portability: spreadsheets, migrations, and round-tripping the
asset importer. It respects the same list filters as the UI and includes
importer columns plus `status` and a JSON `attributes` column.

**Disaster recovery** is a database-level concern. Take backups with
`pg_dump` (or your hosting provider's managed backup). Application
exports are not a substitute for point-in-time recovery: they omit
tickets, users, attachments, lifecycle history, and the rest of the
workspace state.

---

## 8. Where to look for things

| Concern | File |
|---|---|
| High-level architecture | `CLAUDE.md` |
| Roadmap and feature priorities | `docs/roadmap.md` |
| Feature catalog (what's shipped) | `docs/FEATURE_CATALOG.md` |
| Design plans for in-flight work | `docs/plans/` |
| Backend route registration | `backend/src/main.rs` |
| Postgres migrations | `backend/migrations/` |
| Generated schema | `backend/src/schema.rs` (don't edit by hand) |
| Frontend routes | `frontend/src/router/index.ts` |
| i18n catalogues | `i18n/locales/` (shared, see `i18n/README.md`) |
