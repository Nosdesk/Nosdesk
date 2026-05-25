-- Item C / W4 (D2 + D3): column-level redaction in the audit trigger.
--
-- The original audit_log_trigger() captured the entire row into
-- before_jsonb / after_jsonb. For tier-3 tables that hold PII
-- (D2: users.name, user_emails.email, ...) or encrypted credentials
-- and token hashes (D3: channel_credentials.encrypted_value,
-- api_tokens.token_hash, users.mfa_secret, ...) that means the
-- sensitive value lands in the audit log itself.
--
-- This rewrite makes the function take a column-exclusion list as
-- additional trigger arguments. TG_ARGV[0] still names the primary-key
-- column; every argument after it names a column whose VALUE must be
-- dropped from the diff. For each excluded column the diff instead
-- carries a synthetic `<col>_changed` boolean, so an auditor can prove
-- *whether* the value changed (D3 equality-auditability) without
-- recovering it, and the real column name still appears in changed_cols
-- (D2: the diff records that the PII column changed, by whom, when).
--
-- With no extra arguments the function behaves exactly as before
-- (`jsonb - '{}'::text[]` is a no-op and the FOREACH loop is empty), so
-- every trigger already attached keeps its current behaviour until it is
-- re-attached with an exclusion list.

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

COMMENT ON FUNCTION audit_log_trigger() IS
    'Generic INSERT/UPDATE/DELETE audit trigger. Attach with: '
    'CREATE TRIGGER tr_audit_<table> AFTER INSERT OR UPDATE OR DELETE '
    'ON <table> FOR EACH ROW EXECUTE FUNCTION '
    'audit_log_trigger(<pk_column>[, <redacted_column>...]); '
    'The first argument names the primary key column recorded in pk_text. '
    'Any further arguments name columns whose value is dropped from the '
    'before/after diff (PII / encrypted credentials); for each the diff '
    'instead carries a <column>_changed boolean and the column name still '
    'appears in changed_cols.';
