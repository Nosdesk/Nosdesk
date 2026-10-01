-- UUIDv7 column defaults use PostgreSQL 18's native uuidv7() instead of the
-- PL/pgSQL uuid_generate_v7() that stood in for it while Nosdesk ran on
-- PostgreSQL 17. Both produce RFC 9562 version 7 UUIDs; no row changes, only
-- the default each column uses for new rows.
DO $$
BEGIN
    IF current_setting('server_version_num')::int < 180000 THEN
        RAISE EXCEPTION 'Nosdesk needs PostgreSQL 18 or later; this server runs %',
            current_setting('server_version');
    END IF;
END $$;

-- Every column defaulting to the old function, sync_actions partitions
-- included (which months exist differs per database, so the catalog says).
DO $$
DECLARE
    col record;
BEGIN
    FOR col IN
        SELECT c.relname AS tbl, a.attname AS col
        FROM pg_attrdef d
        JOIN pg_class c ON c.oid = d.adrelid
        JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_attribute a ON a.attrelid = d.adrelid AND a.attnum = d.adnum
        WHERE n.nspname = 'public'
          AND pg_get_expr(d.adbin, d.adrelid) LIKE '%uuid_generate_v7()%'
        ORDER BY 1, 2
    LOOP
        EXECUTE format('ALTER TABLE public.%I ALTER COLUMN %I SET DEFAULT uuidv7()', col.tbl, col.col);
    END LOOP;
END $$;

-- Nothing references it now; this fails if any default still does.
DROP FUNCTION public.uuid_generate_v7();
