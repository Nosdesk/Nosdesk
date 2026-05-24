-- Backfill: enable + force RLS on every existing partition child of
-- audit_log and sync_actions, and attach the same workspace-isolation
-- policy as the parent.
--
-- Why this is necessary (security correctness):
-- `CREATE TABLE child (LIKE parent INCLUDING ALL)` copies columns,
-- constraints, indexes, defaults, storage parameters, and comments,
-- but NOT row-security state (PG docs / CREATE TABLE LIKE). After
-- ATTACH PARTITION, the child has no RLS enabled. Two consequences:
--
-- 1. Direct queries against a child partition (e.g.
--    `SELECT * FROM audit_log_2026_07`) bypass the parent's policy
--    entirely. A non-superuser nosdesk_app role with blanket
--    SELECT grants can read everything in that child.
--
-- 2. INSERTs via the parent route to a child by partition key;
--    Postgres evaluates BOTH the parent's WITH CHECK and the
--    target child's WITH CHECK. With RLS off on the child, only
--    the parent's check applies — fine for inserts, but the
--    direct-SELECT bypass remains.
--
-- Fix: enable + force RLS on each child, AND replicate the parent's
-- policy on each child so:
--   - direct-child SELECT is filtered by workspace_id (correct)
--   - direct-child INSERT WITH CHECK enforces workspace_id (correct)
--   - parent-routed SELECT continues to work (both parent and child
--     policies pass with the same shape)
--   - parent-routed INSERT continues to work (both WITH CHECKs pass)
--
-- The partition rotation code (backend/src/sync/partitions.rs) gets
-- the same treatment in this commit, so future partitions get RLS
-- attached automatically.

DO $$
DECLARE
    parent_name text;
    child_name text;
    policy_name text;
    using_predicate constant text :=
        'workspace_id = NULLIF(current_setting(''app.workspace_id'', true), '''')::int '
        'OR NULLIF(current_setting(''app.bypass_workspace_check'', true), '''') = ''true''';
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
            policy_name := child_name || '_workspace_isolation';

            EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', child_name);
            EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', child_name);
            EXECUTE format('DROP POLICY IF EXISTS %I ON %I', policy_name, child_name);
            EXECUTE format(
                'CREATE POLICY %I ON %I USING (%s) WITH CHECK (%s)',
                policy_name, child_name, using_predicate, using_predicate
            );

            RAISE NOTICE 'Enabled RLS + workspace_isolation policy on %', child_name;
        END LOOP;
    END LOOP;
END $$;
