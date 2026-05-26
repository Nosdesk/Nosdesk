-- Restore the W4 audit triggers on the auth / identity tables (inverse
-- of up.sql). NOTE: this reintroduces the login / token-refresh
-- breakage described in up.sql, because audit_log.workspace_id is
-- NOT NULL and these tables are written without a workspace GUC. Kept
-- only for migration symmetry.

CREATE TRIGGER tr_audit_active_sessions
    AFTER INSERT OR UPDATE OR DELETE ON active_sessions
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_api_tokens
    AFTER INSERT OR UPDATE OR DELETE ON api_tokens
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id', 'token_hash');

CREATE TRIGGER tr_audit_refresh_tokens
    AFTER INSERT OR UPDATE OR DELETE ON refresh_tokens
    FOR EACH ROW EXECUTE FUNCTION
    audit_log_trigger('id', 'token_hash', 'replaced_by_hash');

CREATE TRIGGER tr_audit_reset_tokens
    AFTER INSERT OR UPDATE OR DELETE ON reset_tokens
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('token_hash');

CREATE TRIGGER tr_audit_passkey_credentials
    AFTER INSERT OR UPDATE OR DELETE ON passkey_credentials
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_user_emails
    AFTER INSERT OR UPDATE OR DELETE ON user_emails
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id', 'email');

CREATE TRIGGER tr_audit_user_auth_identities
    AFTER INSERT OR UPDATE OR DELETE ON user_auth_identities
    FOR EACH ROW EXECUTE FUNCTION
    audit_log_trigger('id', 'password_hash', 'email', 'metadata');
