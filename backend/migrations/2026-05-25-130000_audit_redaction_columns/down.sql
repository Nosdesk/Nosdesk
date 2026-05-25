-- Restore the pre-W4 audit_log_trigger() that captures the whole row
-- with no column exclusion. (The companion attach migration's down
-- must run first so no trigger is still passing exclusion arguments.)

CREATE OR REPLACE FUNCTION audit_log_trigger() RETURNS TRIGGER AS $$
DECLARE
    actor UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
    corr  UUID := NULLIF(current_setting('app.correlation_id', true), '')::UUID;
    pk    TEXT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        INSERT INTO audit_log (table_name, pk_text, op, after_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'I', to_jsonb(NEW), actor, corr);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING NEW;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id)
        VALUES (
            TG_TABLE_NAME,
            pk,
            'U',
            to_jsonb(OLD),
            to_jsonb(NEW),
            ARRAY(
                SELECT k FROM jsonb_each(to_jsonb(NEW)) e1(k, v1)
                WHERE to_jsonb(NEW) -> e1.k IS DISTINCT FROM to_jsonb(OLD) -> e1.k
            ),
            actor,
            corr
        );
        RETURN NEW;
    ELSE
        EXECUTE format('SELECT ($1).%I::TEXT', TG_ARGV[0]) INTO pk USING OLD;
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, actor_uuid, correlation_id)
        VALUES (TG_TABLE_NAME, pk, 'D', to_jsonb(OLD), actor, corr);
        RETURN OLD;
    END IF;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION audit_log_trigger() IS
    'Generic INSERT/UPDATE/DELETE audit trigger. Attach with: '
    'CREATE TRIGGER tr_audit_<table> AFTER INSERT OR UPDATE OR DELETE '
    'ON <table> FOR EACH ROW EXECUTE FUNCTION audit_log_trigger(<pk_column_name>); '
    'The trigger argument names the primary key column to record in pk_text.';
