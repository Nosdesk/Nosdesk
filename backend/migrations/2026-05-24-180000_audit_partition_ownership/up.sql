-- Phase 3i.4 - transfer audit_log and sync_actions ownership to
-- nosdesk_admin so the partition rotator can run as the BYPASSRLS
-- role instead of the superuser login.
--
-- Today these tables (parents + every monthly child + the _default
-- parachute + the id sequences) are owned by `nosdesk`, the
-- superuser the deploy pipeline uses to run migrations. The
-- runtime scheduler also connects as `nosdesk` and that's why the
-- partition rotator's DDL (CREATE TABLE LIKE, ATTACH PARTITION,
-- CREATE POLICY, REVOKE) works at all - it's silently piggy-
-- backing on superuser.
--
-- Problem: any future change that drops the scheduler to a less-
-- privileged role (e.g. a side-car partition-maintenance job that
-- connects as `nosdesk_app` and SETs to `nosdesk_admin`, or a
-- read-replica promotion that limits the migration role's
-- privileges) silently fails at the DDL step. Plus, the implicit
-- "rotator needs superuser" coupling is invisible until something
-- breaks production.
--
-- Fix: hand ownership of the partition trees to `nosdesk_admin`
-- (the NOLOGIN BYPASSRLS elevation target). Then:
--
--   * The rotator can `SET LOCAL ROLE nosdesk_admin` and do all
--     its DDL without superuser. Today's deploy still works because
--     superuser owns nothing it needs to touch.
--   * Direct DDL by `nosdesk` still works (superusers override
--     ownership checks).
--   * `nosdesk_app` still cannot DDL these tables (it inherits
--     nothing from nosdesk_admin per the 3h.6 INHERIT FALSE grant)
--     so the defense-in-depth REVOKEs from 3h.6 stay meaningful.
--
-- What this migration changes:
--   1. Parent tables: audit_log, sync_actions.
--   2. Every existing child partition (monthly children + _default).
--   3. The id sequences referenced by the parents' DEFAULT clauses.
--
-- Indexes follow their table owner automatically - ALTER TABLE
-- OWNER TO cascades to attached indexes - so no explicit index
-- reassignment is needed.
--
-- Note that ALTER TABLE OWNER TO on a partitioned PARENT does
-- NOT cascade to the children. Each partition needs its own
-- ALTER. The DO block iterates pg_inherits to catch every child
-- regardless of naming convention or how many monthly partitions
-- have been provisioned.

-- 1. Parent tables.
ALTER TABLE audit_log OWNER TO nosdesk_admin;
ALTER TABLE sync_actions OWNER TO nosdesk_admin;

-- 2. Every existing child partition (monthly + _default).
DO $$
DECLARE
    parent_name text;
    child_name text;
BEGIN
    FOR parent_name IN
        SELECT unnest(ARRAY['audit_log', 'sync_actions'])
    LOOP
        FOR child_name IN
            SELECT c.relname
            FROM pg_inherits i
            JOIN pg_class c ON i.inhrelid = c.oid
            WHERE i.inhparent = parent_name::regclass
        LOOP
            EXECUTE format(
                'ALTER TABLE %I OWNER TO nosdesk_admin',
                child_name
            );
        END LOOP;
    END LOOP;
END
$$;

-- 3. Id sequences. The parents declare `id BIGSERIAL` (or similar),
--    which provisions an OWNED-BY sequence. Reassign so the rotator
--    can extend / reset them if a future operation requires it.
DO $$
DECLARE
    seq_name text;
BEGIN
    FOR seq_name IN
        SELECT c.relname
        FROM pg_class c
        WHERE c.relkind = 'S'
          AND (
              c.relname LIKE 'audit_log_%seq'
              OR c.relname LIKE 'sync_actions_%seq'
          )
    LOOP
        EXECUTE format(
            'ALTER SEQUENCE %I OWNER TO nosdesk_admin',
            seq_name
        );
    END LOOP;
END
$$;
