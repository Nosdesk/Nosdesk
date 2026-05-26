-- Item C / W5b (D5): recursive-audit-read guard.
--
-- The unified audit endpoint opens its transaction with
-- `SET LOCAL nosdesk.in_audit_read = 'true'` and emits exactly one
-- tier-1 `data.audit.read` event itself. To guarantee that reading the
-- audit log can never spawn further audit_log rows (the Supabase
-- recursive-loop failure mode), the trigger short-circuits whenever
-- that flag is set: any write to an audited table inside an audit-read
-- transaction produces no audit_log row.
--
-- This rebuilds the W4 (2026-05-25-130000) function verbatim and only
-- prepends the guard; the column-redaction behaviour is unchanged.

CREATE OR REPLACE FUNCTION audit_log_trigger() RETURNS TRIGGER AS $$
DECLARE
    actor    UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
    corr     UUID := NULLIF(current_setting('app.correlation_id', true), '')::UUID;
    pk       TEXT;
    excluded TEXT[] := ARRAY[]::TEXT[];
    col      TEXT;
    i        INT;
    old_j    JSONB;
    new_j    JSONB;
    before_j JSONB;
    after_j  JSONB;
BEGIN
    -- D5: suppress audit capture for writes made inside an audit-read
    -- transaction. Reads de-duplicate to the single data.audit.read
    -- event the handler emits via emit::record (a different table).
    IF current_setting('nosdesk.in_audit_read', true) = 'true' THEN
        RETURN COALESCE(NEW, OLD);
    END IF;

    -- Collect the excluded-column names (TG_ARGV is 0-based; index 0 is
    -- the PK column, 1..N-1 are the columns to redact).
    IF TG_NARGS > 1 THEN
        FOR i IN 1 .. (TG_NARGS - 1) LOOP
            excluded := array_append(excluded, TG_ARGV[i]);
        END LOOP;
    END IF;

    IF TG_OP = 'INSERT' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        new_j := to_jsonb(NEW);
        after_j := new_j - excluded;
        FOREACH col IN ARRAY excluded LOOP
            -- `->>` yields SQL NULL for a JSON-null / absent value
            -- (unlike `->`, which returns the JSONB value 'null'), so
            -- this reads as "a non-null value was provided".
            after_j := after_j
                || jsonb_build_object(col || '_changed', (new_j ->> col) IS NOT NULL);
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, after_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'I', after_j, actor, corr);
        RETURN NEW;

    ELSIF TG_OP = 'UPDATE' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        old_j := to_jsonb(OLD);
        new_j := to_jsonb(NEW);
        before_j := old_j - excluded;
        after_j := new_j - excluded;
        FOREACH col IN ARRAY excluded LOOP
            after_j := after_j
                || jsonb_build_object(col || '_changed', (old_j -> col) IS DISTINCT FROM (new_j -> col));
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id)
        VALUES (
            TG_TABLE_NAME,
            pk,
            'U',
            before_j,
            after_j,
            -- changed_cols is computed from the full rows so an excluded
            -- column that changed is still named here (just not valued).
            ARRAY(
                SELECT k FROM jsonb_each(new_j) e1(k, v1)
                WHERE new_j -> e1.k IS DISTINCT FROM old_j -> e1.k
            ),
            actor,
            corr
        );
        RETURN NEW;

    ELSE
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING OLD;
        old_j := to_jsonb(OLD);
        before_j := old_j - excluded;
        FOREACH col IN ARRAY excluded LOOP
            -- `->>` (not `->`) so a JSON-null column reads as "had no
            -- value", matching the INSERT branch.
            before_j := before_j
                || jsonb_build_object(col || '_changed', (old_j ->> col) IS NOT NULL);
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'D', before_j, actor, corr);
        RETURN OLD;
    END IF;
END;
$$ LANGUAGE plpgsql;
