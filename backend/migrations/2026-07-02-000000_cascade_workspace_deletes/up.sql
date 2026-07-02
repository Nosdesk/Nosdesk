-- Workspace hard-delete must purge the workspace's tenant data. Tenant tables
-- added more recently already declare ON DELETE CASCADE on their workspaces FK
-- (bug_reports, rules, user_profiles, yjs_snapshots, ldap, ...); the
-- initial-schema tables were left ON DELETE NO ACTION, so deleting a populated
-- (or merely seeded) workspace failed with a foreign-key violation. Bring every
-- workspace FK in line so a single DELETE FROM workspaces cascades through the
-- workspace's data. Partition children inherit the parent's FK, so only
-- non-partition tables are altered here.
DO $$
DECLARE
    r RECORD;
BEGIN
    FOR r IN
        SELECT c.conname, c.conrelid::regclass AS tbl
        FROM pg_constraint c
        JOIN pg_class cl ON cl.oid = c.conrelid
        WHERE c.contype = 'f'
          AND c.confrelid = 'public.workspaces'::regclass
          AND c.confdeltype = 'a'
          AND NOT cl.relispartition
    LOOP
        EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I', r.tbl, r.conname);
        EXECUTE format(
            'ALTER TABLE %s ADD CONSTRAINT %I FOREIGN KEY (workspace_id) '
            'REFERENCES public.workspaces(id) ON DELETE CASCADE',
            r.tbl, r.conname);
    END LOOP;
END $$;
