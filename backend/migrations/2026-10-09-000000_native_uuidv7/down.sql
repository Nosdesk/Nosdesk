-- Restore the PL/pgSQL uuid_generate_v7() (as in the initial schema) and point
-- the columns using native uuidv7() back at it.
CREATE FUNCTION public.uuid_generate_v7() RETURNS uuid
    LANGUAGE plpgsql
    AS $$
DECLARE
    unix_ts_ms BIGINT;
    rand_bytes BYTEA;
    ts_hex TEXT;
BEGIN
    -- Get current timestamp in milliseconds since Unix epoch
    unix_ts_ms := (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT;

    -- Build 6-byte timestamp as hex (12 hex chars)
    ts_hex := LPAD(TO_HEX(unix_ts_ms), 12, '0');

    -- Generate 10 random bytes for version, variant, and random portions
    rand_bytes := gen_random_bytes(10);

    -- Set version (4 bits = 0x7) in first random byte
    rand_bytes := SET_BYTE(rand_bytes, 0, (GET_BYTE(rand_bytes, 0) & 15) | 112);

    -- Set variant (2 bits = 0b10) in third random byte (position 8 in UUID)
    rand_bytes := SET_BYTE(rand_bytes, 2, (GET_BYTE(rand_bytes, 2) & 63) | 128);

    RETURN CAST(ts_hex || ENCODE(rand_bytes, 'hex') AS UUID);
END;
$$;

ALTER FUNCTION public.uuid_generate_v7() OWNER TO nosdesk_admin;

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
          AND pg_get_expr(d.adbin, d.adrelid) LIKE '%uuidv7()%'
        ORDER BY 1, 2
    LOOP
        EXECUTE format('ALTER TABLE public.%I ALTER COLUMN %I SET DEFAULT public.uuid_generate_v7()', col.tbl, col.col);
    END LOOP;
END $$;
