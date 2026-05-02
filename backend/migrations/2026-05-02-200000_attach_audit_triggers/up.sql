-- Attach the generic audit_log_trigger() function to tier-3 tables.
-- Tier-3 = "compliance / forensics only" — no sync client subscribes
-- to events on these. The trigger writes a JSON-diff row to audit_log
-- on every INSERT / UPDATE / DELETE, attributing the change to the
-- session-local actor GUC if set.
--
-- Selection rationale (per architecture doc § 5):
-- - site_settings: workspace config edits worth a forensic trail.
-- - plugin_data, plugin_collection_rows: plugin local storage.
-- - webhook_deliveries: high-volume operational, but compliance can
--   ask "did we ever send notification X to webhook Y?"
-- - user_ticket_views: tracks who looked at what; audit-only because
--   no sync client cares about another user's view history.
--
-- NOT attached (kept bespoke for now):
-- - active_sessions, refresh_tokens, reset_tokens, api_tokens — these
--   already write to security_events on meaningful state changes.
-- - search_query_log — high-volume, no compliance need.
-- - sync_history — operational metadata for Microsoft Graph imports.
-- - backup_jobs — own bespoke job-state table.

CREATE TRIGGER tr_audit_site_settings
    AFTER INSERT OR UPDATE OR DELETE ON site_settings
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_plugin_data
    AFTER INSERT OR UPDATE OR DELETE ON plugin_data
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_plugin_collection_rows
    AFTER INSERT OR UPDATE OR DELETE ON plugin_collection_rows
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_webhook_deliveries
    AFTER INSERT OR UPDATE OR DELETE ON webhook_deliveries
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_user_ticket_views
    AFTER INSERT OR UPDATE OR DELETE ON user_ticket_views
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('user_uuid');
