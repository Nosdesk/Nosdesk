-- Reverse: hand ownership back to the deploy-pipeline role
-- (default: the same superuser the up migration runs as, which is
-- whatever `current_user` returns at down-time). For dev this is
-- `nosdesk`; for production it's the connection's CURRENT_USER. We
-- avoid hardcoding a literal so down works regardless of who's
-- running it.
--
-- Same sweep + filter as up.sql, in reverse.

DO $$
DECLARE
    revert_owner text := current_user;
    target_table text;
BEGIN
    FOR target_table IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON c.relnamespace = n.oid
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE n.nspname = 'public'
          AND c.relkind IN ('r', 'p')
          AND a.attname = 'workspace_id'
          AND a.attnum > 0
          AND NOT a.attisdropped
          AND c.relname NOT IN ('audit_log', 'sync_actions')
    LOOP
        EXECUTE format(
            'ALTER TABLE %I OWNER TO %I',
            target_table, revert_owner
        );
    END LOOP;
END
$$;

DO $$
DECLARE
    revert_owner text := current_user;
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
            'ALTER SEQUENCE %I OWNER TO %I',
            seq_name, revert_owner
        );
    END LOOP;
END
$$;

DO $$
DECLARE
    revert_owner text := current_user;
    parent_name text;
    child_name text;
BEGIN
    FOR parent_name IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON c.relnamespace = n.oid
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE n.nspname = 'public'
          AND c.relkind = 'p'
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
                'ALTER TABLE %I OWNER TO %I',
                child_name, revert_owner
            );
        END LOOP;
    END LOOP;
END
$$;
