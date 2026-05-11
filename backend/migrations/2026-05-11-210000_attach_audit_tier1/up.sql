-- Item C — attach audit_log_trigger to tier-1 entities.
--
-- Tier-1 = "operator-facing surfaces a sysadmin actually wants a forensic
-- trail for". The original 2026-05-02-200000 migration attached the trigger
-- only to plugin_data / site_settings / webhook_deliveries / user_ticket_views
-- (tier-3 forensics). This extends coverage to the entities admins care about.
--
-- Selection rationale:
-- - tickets:           "who closed this and when?"
-- - users:             role / pronoun / theme changes; account provisioning
-- - groups:            membership/scope changes for compliance
-- - ticket_categories: visibility + active/inactive toggles
-- - workflow_states:   workflow edits affect every ticket downstream
-- - assignment_rules:  who set the rule that auto-routed this ticket?
-- - sla_policies:      changes here invalidate historical SLA reporting
-- - webhooks:          high-impact integration mutations
--
-- Deliberately NOT attached:
-- - api_tokens, active_sessions, refresh_tokens, reset_tokens — already
--   write to security_events; double-attribution is noise.
-- - documentation_pages, comments, attachments — the user-facing edit
--   history (revision tables, edited_at) covers the same ground.
-- - high-volume operational tables (search_query_log, sync_history).
--
-- The trigger fires on every INSERT/UPDATE/DELETE for the row's lifetime;
-- benchmark before/after on the hot path (tickets) before relying on this
-- under heavy load. If the cost is measurable, restrict the trigger to
-- specific operations (UPDATE OR DELETE only) since INSERTs are already
-- observable from created_at.

CREATE TRIGGER tr_audit_tickets
    AFTER INSERT OR UPDATE OR DELETE ON tickets
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_users
    AFTER INSERT OR UPDATE OR DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('uuid');

CREATE TRIGGER tr_audit_groups
    AFTER INSERT OR UPDATE OR DELETE ON groups
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_ticket_categories
    AFTER INSERT OR UPDATE OR DELETE ON ticket_categories
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_workflow_states
    AFTER INSERT OR UPDATE OR DELETE ON workflow_states
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_assignment_rules
    AFTER INSERT OR UPDATE OR DELETE ON assignment_rules
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_sla_policies
    AFTER INSERT OR UPDATE OR DELETE ON sla_policies
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_webhooks
    AFTER INSERT OR UPDATE OR DELETE ON webhooks
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');
