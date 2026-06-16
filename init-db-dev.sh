#!/bin/bash
# DEV-ONLY runtime-role bootstrap. Mounted into the postgres container by
# compose.dev.yaml only; production never runs this.
#
# Why: the hosted runtime connects as `nosdesk_app` (NOBYPASSRLS), so RLS is
# actually enforced. The default dev/self-hosted DATABASE_URL connects as the
# `nosdesk` superuser (BYPASSRLS), which silently masks every RLS-scoping bug.
# To make dev mirror hosted, the dev backend connects as `nosdesk_app` too
# (see DATABASE_URL in compose.dev.yaml).
#
# The initial_schema migration creates `nosdesk_app` as NOLOGIN behind an
# `IF NOT EXISTS` guard, so pre-creating it here as a LOGIN role wins and the
# migration leaves it alone. Migrations still run as the superuser via
# MIGRATION_DATABASE_URL.
#
# Runs after 01-init-db.sh, only on a fresh data dir (first `make dev` or after
# `make clean`). For an already-initialised dev DB, run `make dev-app-role`.
set -euo pipefail

NOSDESK_APP_PASSWORD="${NOSDESK_APP_PASSWORD:-nosdesk_app_dev}"

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<EOSQL
    DO \$\$
    BEGIN
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'nosdesk_app') THEN
            ALTER ROLE nosdesk_app LOGIN PASSWORD '${NOSDESK_APP_PASSWORD}';
        ELSE
            CREATE ROLE nosdesk_app LOGIN PASSWORD '${NOSDESK_APP_PASSWORD}' NOBYPASSRLS NOINHERIT;
        END IF;
    END
    \$\$;
EOSQL
