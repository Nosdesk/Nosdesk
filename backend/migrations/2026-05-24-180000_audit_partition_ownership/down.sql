-- Reverse Phase 3i.4: hand audit_log / sync_actions ownership back
-- to the migration runner role (nosdesk). Same iteration shape as
-- up.sql; ALTER TABLE OWNER TO must run on each partition and
-- sequence individually.

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
            'ALTER SEQUENCE %I OWNER TO nosdesk',
            seq_name
        );
    END LOOP;
END
$$;

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
                'ALTER TABLE %I OWNER TO nosdesk',
                child_name
            );
        END LOOP;
    END LOOP;
END
$$;

ALTER TABLE sync_actions OWNER TO nosdesk;
ALTER TABLE audit_log OWNER TO nosdesk;
