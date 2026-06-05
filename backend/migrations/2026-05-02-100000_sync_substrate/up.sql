-- Phase 2 substrate: sync_actions (typed semantic events), audit_log
-- (generic JSON-diff catch-all with an actor-aware trigger), and
-- system_meta (instance bookkeeping).
--
-- Both event tables are RANGE-partitioned by occurred_at on a monthly
-- cadence. We provision May to August 2026 manually here so the
-- substrate is usable from the moment this migration runs; pg_partman
-- (or a periodic Rust task) takes over provisioning future months in a
-- follow-up.
--
-- UUIDv7 (time-sortable, RFC 9562) is the canonical event identifier.
-- Generated via the custom uuid_generate_v7() function (initial-schema
-- migration), which works on any Postgres version, so the stack runs on
-- Fly Managed Postgres 17 without depending on the PG18 uuidv7() builtin.

-- ---------------------------------------------------------------------------
-- sync_actions
-- ---------------------------------------------------------------------------

CREATE TYPE sync_op AS ENUM ('I', 'U', 'D', 'A');
COMMENT ON TYPE sync_op IS
    'I=insert, U=update, D=delete, A=archive (soft delete)';

CREATE TYPE sync_aggregate AS ENUM (
    'ticket',
    'project',
    'project_ticket',
    'workflow_state',
    'comment',
    'attachment',
    'assignment',
    'group_membership'
);

CREATE TABLE sync_actions (
    sync_id        BIGSERIAL,
    event_uuid     UUID NOT NULL DEFAULT uuid_generate_v7(),
    aggregate      sync_aggregate NOT NULL,
    aggregate_id   TEXT NOT NULL,
    op             sync_op NOT NULL,
    event_type     VARCHAR(64) NOT NULL,
    schema_version SMALLINT NOT NULL DEFAULT 1,
    data           JSONB NOT NULL,
    groups         TEXT[] NOT NULL,
    actor_uuid     UUID,
    actor_kind     VARCHAR(16) NOT NULL DEFAULT 'user',
    actor_ref      TEXT,
    correlation_id UUID,
    causation_id   UUID,
    client_tx_id   TEXT,
    occurred_at    TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (sync_id, occurred_at)
) PARTITION BY RANGE (occurred_at);

CREATE TABLE sync_actions_2026_05 PARTITION OF sync_actions
    FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
CREATE TABLE sync_actions_2026_06 PARTITION OF sync_actions
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
CREATE TABLE sync_actions_2026_07 PARTITION OF sync_actions
    FOR VALUES FROM ('2026-07-01') TO ('2026-08-01');
CREATE TABLE sync_actions_2026_08 PARTITION OF sync_actions
    FOR VALUES FROM ('2026-08-01') TO ('2026-09-01');

CREATE INDEX sync_actions_aggregate_idx
    ON sync_actions (aggregate, aggregate_id, occurred_at DESC);
CREATE INDEX sync_actions_groups_gin
    ON sync_actions USING GIN (groups);
-- Idempotency dedup for client retries. Postgres requires every
-- partitioning column in any unique index on a partitioned table, so
-- occurred_at is bundled in. In practice client_tx_id values live for
-- a few seconds at most so the cross-partition false-negative window
-- is negligible (a client retry would have to straddle a month
-- boundary, which would not happen).
CREATE UNIQUE INDEX sync_actions_client_tx_id_idx
    ON sync_actions (client_tx_id, occurred_at) WHERE client_tx_id IS NOT NULL;
CREATE INDEX sync_actions_occurred_at_brin
    ON sync_actions USING BRIN (occurred_at);
CREATE INDEX sync_actions_actor_idx
    ON sync_actions (actor_uuid, occurred_at DESC) WHERE actor_uuid IS NOT NULL;
CREATE INDEX sync_actions_correlation_idx
    ON sync_actions (correlation_id) WHERE correlation_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- audit_log
-- ---------------------------------------------------------------------------

CREATE TABLE audit_log (
    id             BIGSERIAL,
    table_name     TEXT NOT NULL,
    pk_text        TEXT NOT NULL,
    op             CHAR(1) NOT NULL CHECK (op IN ('I', 'U', 'D')),
    before_jsonb   JSONB,
    after_jsonb    JSONB,
    changed_cols   TEXT[],
    actor_uuid     UUID,
    correlation_id UUID,
    occurred_at    TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (id, occurred_at)
) PARTITION BY RANGE (occurred_at);

CREATE TABLE audit_log_2026_05 PARTITION OF audit_log
    FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
CREATE TABLE audit_log_2026_06 PARTITION OF audit_log
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
CREATE TABLE audit_log_2026_07 PARTITION OF audit_log
    FOR VALUES FROM ('2026-07-01') TO ('2026-08-01');
CREATE TABLE audit_log_2026_08 PARTITION OF audit_log
    FOR VALUES FROM ('2026-08-01') TO ('2026-09-01');

CREATE INDEX audit_log_table_pk_idx
    ON audit_log (table_name, pk_text, occurred_at DESC);
CREATE INDEX audit_log_occurred_at_brin
    ON audit_log USING BRIN (occurred_at);
CREATE INDEX audit_log_actor_idx
    ON audit_log (actor_uuid, occurred_at DESC) WHERE actor_uuid IS NOT NULL;

-- Generic actor-aware audit trigger function. Pulls actor_uuid and
-- correlation_id from session-local Postgres GUCs that the Rust
-- request middleware sets on every transaction (`SET LOCAL
-- app.actor_uuid = '<uuid>'`). When the GUCs are unset (background
-- jobs, migrations, psql), both columns are NULL and the row still
-- writes.
--
-- The function is created here but not yet attached to any table.
-- A subsequent migration applies it to the tier-3 audit-only tables
-- once the tier-1 vs tier-3 classification is wired up in code.
CREATE OR REPLACE FUNCTION audit_log_trigger() RETURNS TRIGGER AS $$
DECLARE
    actor UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
    corr  UUID := NULLIF(current_setting('app.correlation_id', true), '')::UUID;
    pk    TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        INSERT INTO audit_log (table_name, pk_text, op, after_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'I', to_jsonb(NEW), actor, corr);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id)
        VALUES (
            TG_TABLE_NAME,
            pk,
            'U',
            to_jsonb(OLD),
            to_jsonb(NEW),
            ARRAY(
                SELECT k FROM jsonb_each(to_jsonb(NEW)) e1(k, v1)
                WHERE to_jsonb(NEW) -> e1.k IS DISTINCT FROM to_jsonb(OLD) -> e1.k
            ),
            actor,
            corr
        );
        RETURN NEW;
    ELSE
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING OLD;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'D', to_jsonb(OLD), actor, corr);
        RETURN OLD;
    END IF;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION audit_log_trigger() IS
    'Generic INSERT/UPDATE/DELETE audit trigger. Attach with: '
    'CREATE TRIGGER tr_audit_<table> AFTER INSERT OR UPDATE OR DELETE '
    'ON <table> FOR EACH ROW EXECUTE FUNCTION audit_log_trigger(<pk_column_name>); '
    'The trigger argument names the primary key column to record in pk_text.';

-- ---------------------------------------------------------------------------
-- system_meta
-- ---------------------------------------------------------------------------

CREATE TABLE system_meta (
    key        TEXT PRIMARY KEY,
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO system_meta (key, value) VALUES
    ('schema_hash', '""'::jsonb),
    ('sync_id_high_water', '0'::jsonb),
    ('partition_max_provisioned', '"2026-09-01"'::jsonb);
