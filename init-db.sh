#!/bin/bash
# First-boot database setup. Executed by the postgres official image
# from `/docker-entrypoint-initdb.d/` when the data directory is
# empty (first `docker compose up`, or after `down -v`). Subsequent
# starts skip everything in this directory.
#
# Notes:
#   - $POSTGRES_USER already owns $POSTGRES_DB; no `GRANT ALL` needed
#     on the primary database.
#   - The test database is created with OWNER instead of a post-hoc
#     GRANT so future schema changes don't need permission patching
#     (`CREATE TABLE` etc. inherits owner privileges).
#   - `ON_ERROR_STOP=1` makes psql exit non-zero on the first failure
#     so the entrypoint surfaces the error instead of marking the
#     container healthy with a half-initialised DB.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    -- Separate database for the Rust test suite. Tests wrap each run
    -- in a rolled-back transaction for row isolation, but PostgreSQL
    -- sequences are non-transactional. Sharing the dev DB would burn
    -- IDs out of ticket/user/comment sequences and make IDs jump
    -- into the thousands after a few cargo test runs.
    CREATE DATABASE ${POSTGRES_DB}_test OWNER "$POSTGRES_USER";
EOSQL
