-- Reverse the role-based bypass migration: restore the bypass-
-- disjunct shape on every workspace_isolation policy, then revoke
-- and drop `nosdesk_admin`.

DO $$
DECLARE
    pol record;
    predicate constant text :=
        'workspace_id = NULLIF(current_setting(''app.workspace_id'', true), '''')::int '
        'OR NULLIF(current_setting(''app.bypass_workspace_check'', true), '''') = ''true''';
BEGIN
    FOR pol IN
        SELECT schemaname, tablename, policyname
        FROM pg_policies
        WHERE schemaname = 'public' AND policyname LIKE '%_workspace_isolation'
    LOOP
        EXECUTE format('DROP POLICY %I ON %I.%I',
            pol.policyname, pol.schemaname, pol.tablename);
        EXECUTE format(
            'CREATE POLICY %I ON %I.%I USING (%s) WITH CHECK (%s)',
            pol.policyname, pol.schemaname, pol.tablename,
            predicate, predicate
        );
    END LOOP;
END
$$;

REVOKE nosdesk_admin FROM nosdesk_app;

ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE SELECT, INSERT, UPDATE, DELETE ON TABLES FROM nosdesk_admin;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    REVOKE USAGE, SELECT, UPDATE ON SEQUENCES FROM nosdesk_admin;
REVOKE USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public FROM nosdesk_admin;
REVOKE SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public FROM nosdesk_admin;
REVOKE USAGE ON SCHEMA public FROM nosdesk_admin;
DROP ROLE IF EXISTS nosdesk_admin;
