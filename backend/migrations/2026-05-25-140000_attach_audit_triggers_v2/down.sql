-- Drop the W4 triggers and restore the users trigger to its pre-W4,
-- no-redaction form. Run before the 2026-05-25-130000 down (which
-- restores the function signature).

DROP TRIGGER IF EXISTS tr_audit_active_sessions ON active_sessions;
DROP TRIGGER IF EXISTS tr_audit_api_tokens ON api_tokens;
DROP TRIGGER IF EXISTS tr_audit_refresh_tokens ON refresh_tokens;
DROP TRIGGER IF EXISTS tr_audit_reset_tokens ON reset_tokens;
DROP TRIGGER IF EXISTS tr_audit_passkey_credentials ON passkey_credentials;
DROP TRIGGER IF EXISTS tr_audit_user_emails ON user_emails;
DROP TRIGGER IF EXISTS tr_audit_user_auth_identities ON user_auth_identities;
DROP TRIGGER IF EXISTS tr_audit_channel_credentials ON channel_credentials;
DROP TRIGGER IF EXISTS tr_audit_category_group_visibility ON category_group_visibility;
DROP TRIGGER IF EXISTS tr_audit_documentation_page_visibility ON documentation_page_visibility;
DROP TRIGGER IF EXISTS tr_audit_documentation_collection_visibility ON documentation_collection_visibility;
DROP TRIGGER IF EXISTS tr_audit_assignment_rule_state ON assignment_rule_state;
DROP TRIGGER IF EXISTS tr_audit_canned_responses ON canned_responses;
DROP TRIGGER IF EXISTS tr_audit_notification_preferences ON notification_preferences;
DROP TRIGGER IF EXISTS tr_audit_plugin_registry_state ON plugin_registry_state;

DROP TRIGGER IF EXISTS tr_audit_users ON users;
CREATE TRIGGER tr_audit_users
    AFTER INSERT OR UPDATE OR DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('uuid');
