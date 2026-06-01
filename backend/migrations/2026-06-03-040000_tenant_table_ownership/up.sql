-- =====================================================================
-- Transfer ownership of every tenant table to `nosdesk_admin`
-- (M5 product-side handoff Task 9 — defense-in-depth).
--
-- Not a correctness fix: `FORCE ROW LEVEL SECURITY` already
-- prevents bypass even by the table owner, so today's situation
-- (most tenant tables owned by the migration superuser) is safe.
-- This migration eliminates the implicit "owner happens to be the
-- migration superuser" assumption so future operations that drop
-- to a less-privileged role can still own the DDL they need.
--
-- Pattern mirrors the audit_log + sync_actions ownership transfer
-- in `2026-05-24-180000_audit_partition_ownership`, but generalised
-- across every public-schema table with a `workspace_id` column.
-- Idempotent: re-running is a no-op (Postgres tolerates an OWNER TO
-- the current owner).
--
-- Caveats:
--   * `workspaces` itself is intentionally excluded — it's a global
--     table (no `workspace_id` column on itself), and ownership of
--     the bootstrap row is operator-grade. Same for `users` and the
--     other identity-layer tables; they don't carry workspace_id.
--   * `__diesel_schema_migrations` is filtered by the workspace_id
--     gate; it has no such column.
--   * Sequences owned by these tables (`<table>_id_seq` etc.) are
--     reassigned in the second DO block. Indexes inherit the
--     parent table's owner automatically — no explicit transfer
--     needed.
-- =====================================================================

DO $$
DECLARE
    target_table text;
BEGIN
    FOR target_table IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON c.relnamespace = n.oid
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE n.nspname = 'public'
          AND c.relkind IN ('r', 'p')  -- ordinary + partitioned tables
          AND a.attname = 'workspace_id'
          AND a.attnum > 0
          AND NOT a.attisdropped
          -- audit_log + sync_actions parents already transferred in
          -- 2026-05-24-180000. Re-issuing OWNER TO is a no-op, but
          -- the partition children are also already correct so this
          -- filter just makes the log quieter.
          AND c.relname NOT IN ('audit_log', 'sync_actions')
    LOOP
        EXECUTE format(
            'ALTER TABLE %I OWNER TO nosdesk_admin',
            target_table
        );
    END LOOP;
END
$$;

-- Sequences owned by transferred tables. We can't filter cheaply
-- on "owned by a tenant table" so we iterate every sequence whose
-- owning table now has workspace_id; pg_depend ties them together.
DO $$
DECLARE
    seq_name text;
BEGIN
    FOR seq_name IN
        SELECT s.relname
        FROM pg_class s
        JOIN pg_depend d ON d.objid = s.oid AND d.classid = 'pg_class'::regclass
        JOIN pg_class t ON d.refobjid = t.oid
        JOIN pg_attribute a ON a.attrelid = t.oid
            AND a.attname = 'workspace_id'
            AND a.attnum > 0
            AND NOT a.attisdropped
        JOIN pg_namespace n ON s.relnamespace = n.oid
        WHERE s.relkind = 'S'
          AND n.nspname = 'public'
          AND t.relname NOT IN ('audit_log', 'sync_actions')
    LOOP
        EXECUTE format(
            'ALTER SEQUENCE %I OWNER TO nosdesk_admin',
            seq_name
        );
    END LOOP;
END
$$;

-- Partition children of any partitioned tenant table inherit the
-- parent's owner from `ALTER TABLE ... OWNER TO`? No — partitions
-- are independent objects in Postgres, exactly like the audit_log /
-- sync_actions case above. Sweep them here too.
DO $$
DECLARE
    parent_name text;
    child_name text;
BEGIN
    FOR parent_name IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON c.relnamespace = n.oid
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE n.nspname = 'public'
          AND c.relkind = 'p'  -- partitioned table parent
          AND a.attname = 'workspace_id'
          AND a.attnum > 0
          AND NOT a.attisdropped
          AND c.relname NOT IN ('audit_log', 'sync_actions')
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
