-- Restore the W4 (2026-05-25-130000) function without the
-- nosdesk.in_audit_read short-circuit. Column redaction is preserved.

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
            before_j := before_j
                || jsonb_build_object(col || '_changed', (old_j ->> col) IS NOT NULL);
        END LOOP;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'D', before_j, actor, corr);
        RETURN OLD;
    END IF;
END;
$$ LANGUAGE plpgsql;
