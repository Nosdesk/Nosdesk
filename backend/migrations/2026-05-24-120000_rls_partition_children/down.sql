-- Reverse the partition-child RLS backfill: drop the policy and
-- disable RLS on every current partition child of audit_log and
-- sync_actions. Down.sql operates on the current set of partitions
-- (which may differ from the up.sql apply-time set because the
-- rotation cron creates more children over time); that's fine
-- because the operation is idempotent (DROP POLICY IF EXISTS,
-- DISABLE on already-disabled is a no-op).

DO $$
DECLARE
    parent_name text;
    child_name text;
    policy_name text;
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
            EXECUTE format('DROP POLICY IF EXISTS %I ON %I', policy_name, child_name);
            EXECUTE format('ALTER TABLE %I NO FORCE ROW LEVEL SECURITY', child_name);
            EXECUTE format('ALTER TABLE %I DISABLE ROW LEVEL SECURITY', child_name);
        END LOOP;
    END LOOP;
END $$;
