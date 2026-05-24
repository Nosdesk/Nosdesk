-- Reverse the platform-table REVOKEs: restore the blanket-grant
-- access that the POC migration originally established.

REVOKE nosdesk_admin FROM nosdesk_app;
ALTER ROLE nosdesk_app INHERIT;
GRANT nosdesk_admin TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON workspaces TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON workspace_members TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON __diesel_schema_migrations TO nosdesk_app, nosdesk_admin;

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
              AND c.relname NOT LIKE '%_default'
        LOOP
            EXECUTE format(
                'GRANT SELECT, INSERT, UPDATE, DELETE ON %I TO nosdesk_app, nosdesk_admin',
                child_name
            );
        END LOOP;
    END LOOP;
END
$$;
