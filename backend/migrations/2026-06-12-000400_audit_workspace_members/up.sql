-- Audit trail for workspace membership changes (P1.4).
--
-- `workspace_members` had no audit trigger, so grants, role changes, and
-- removals (now self-serve via the Phase 4 W3 endpoints) left no record
-- of who changed whose access. Add a trigger that writes audit_log on
-- every INSERT/UPDATE/DELETE, like the other audited tables.
--
-- Why a specialised function instead of the generic `audit_log_trigger`:
-- that one derives `audit_log.workspace_id` from the `app.workspace_id`
-- GUC and raises NDX01 if it's unset. The operator console writes
-- memberships via a workspace-agnostic BYPASSRLS connection that does
-- not pin the target workspace, so the generic trigger would either
-- fail or attribute the row to the wrong workspace. `workspace_members`
-- carries its workspace on the row, so we read it from NEW/OLD directly:
-- always correct, never dependent on caller discipline. Actor and
-- correlation still come from the GUCs the write path sets (NULL for an
-- automated grant, e.g. first-login projection, which is honest).

CREATE FUNCTION public.audit_workspace_members() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    actor UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
    corr  UUID := NULLIF(current_setting('app.correlation_id', true), '')::UUID;
BEGIN
    -- Mirror the generic trigger: suppress capture inside an audit-read txn.
    IF current_setting('nosdesk.in_audit_read', true) = 'true' THEN
        RETURN COALESCE(NEW, OLD);
    END IF;

    IF TG_OP = 'INSERT' THEN
        INSERT INTO audit_log (table_name, pk_text, op, after_jsonb, actor_uuid, correlation_id, workspace_id)
        VALUES ('workspace_members', NEW.user_uuid::text, 'I', to_jsonb(NEW), actor, corr, NEW.workspace_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, workspace_id)
        VALUES (
            'workspace_members',
            NEW.user_uuid::text,
            'U',
            to_jsonb(OLD),
            to_jsonb(NEW),
            ARRAY(
                SELECT k FROM jsonb_each(to_jsonb(NEW)) e(k, v)
                WHERE to_jsonb(NEW) -> e.k IS DISTINCT FROM to_jsonb(OLD) -> e.k
            ),
            actor,
            corr,
            NEW.workspace_id
        );
        RETURN NEW;
    ELSE
        INSERT INTO audit_log (table_name, pk_text, op, before_jsonb, actor_uuid, correlation_id, workspace_id)
        VALUES ('workspace_members', OLD.user_uuid::text, 'D', to_jsonb(OLD), actor, corr, OLD.workspace_id);
        RETURN OLD;
    END IF;
END;
$$;

ALTER FUNCTION public.audit_workspace_members() OWNER TO nosdesk_admin;

CREATE TRIGGER tr_audit_workspace_members
    AFTER INSERT OR UPDATE OR DELETE ON public.workspace_members
    FOR EACH ROW EXECUTE FUNCTION public.audit_workspace_members();
