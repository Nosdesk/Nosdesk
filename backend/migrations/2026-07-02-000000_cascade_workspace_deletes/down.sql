-- Revert the initial-schema workspace FKs to ON DELETE NO ACTION. Tables that
-- shipped as CASCADE before this migration are excluded so they keep their
-- original behaviour (current CASCADE set = those originals + the ones this
-- migration converted; excluding the originals reverts exactly what up changed).
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
          AND c.confdeltype = 'c'
          AND NOT cl.relispartition
          AND cl.relname NOT IN (
              'bug_reports', 'canned_response_insertions', 'rule_applications',
              'rule_versions', 'rules', 'user_auth_identities', 'yjs_snapshots',
              'user_field_schema', 'user_profiles', 'user_phone_numbers',
              'user_addresses', 'workspace_ldap_settings', 'workspace_ldap_sync_state'
          )
    LOOP
        EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I', r.tbl, r.conname);
        EXECUTE format(
            'ALTER TABLE %s ADD CONSTRAINT %I FOREIGN KEY (workspace_id) '
            'REFERENCES public.workspaces(id)',
            r.tbl, r.conname);
    END LOOP;
END $$;
