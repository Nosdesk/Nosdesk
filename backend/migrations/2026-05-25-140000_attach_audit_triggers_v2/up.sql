-- Item C / W4: attach audit_log_trigger() to the remaining tier-3
-- tables enumerated in event-tier-classification.md, and re-attach the
-- users trigger with a redaction list now that the function supports it
-- (migration 2026-05-25-130000).
--
-- Coverage already in place (left untouched here): tickets, users*,
-- groups, ticket_categories, workflow_states, assignment_rules,
-- sla_policies, webhooks, webhook_deliveries, site_settings, plugins,
-- plugin_data, plugin_collection_rows, plugin_local_signing_key,
-- plugin_trusted_publishers, assets, asset_kinds, asset_audits,
-- asset_usage_log, user_ticket_views.  (* users is re-created below.)
--
-- Intentionally still NOT attached (per the plan's "stay un-triggered"
-- list): notification_rate_limits, notification_types, device_groups,
-- documentation_page_embeddings, documentation_page_tickets,
-- linked_tickets, search_index_state, sync_delta_tokens.

-- --------------------------------------------------------------------
-- Re-attach users with PII (name) + credential (mfa_secret,
-- mfa_backup_codes) redaction. The pre-W4 trigger captured these in the
-- diff; drop and recreate it so they no longer land in audit_log.
-- --------------------------------------------------------------------
DROP TRIGGER IF EXISTS tr_audit_users ON users;
CREATE TRIGGER tr_audit_users
    AFTER INSERT OR UPDATE OR DELETE ON users
    FOR EACH ROW EXECUTE FUNCTION
    audit_log_trigger('uuid', 'name', 'mfa_secret', 'mfa_backup_codes');

-- --------------------------------------------------------------------
-- Auth state. Paired with security_events for the application-level
-- event; the trigger adds the row-level before/after diff. Token hashes
-- are redacted (the raw token is never stored, only its hash, but the
-- hash is still a credential-equivalent and stays out of the diff).
-- --------------------------------------------------------------------
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

-- reset_tokens is keyed by the token hash itself; pk_text therefore
-- holds the hash (one-way, not the raw token) and there is no separate
-- secret column to exclude.
CREATE TRIGGER tr_audit_reset_tokens
    AFTER INSERT OR UPDATE OR DELETE ON reset_tokens
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('token_hash');

CREATE TRIGGER tr_audit_passkey_credentials
    AFTER INSERT OR UPDATE OR DELETE ON passkey_credentials
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

-- --------------------------------------------------------------------
-- Identity / PII tables.
-- --------------------------------------------------------------------
CREATE TRIGGER tr_audit_user_emails
    AFTER INSERT OR UPDATE OR DELETE ON user_emails
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id', 'email');

CREATE TRIGGER tr_audit_user_auth_identities
    AFTER INSERT OR UPDATE OR DELETE ON user_auth_identities
    FOR EACH ROW EXECUTE FUNCTION
    audit_log_trigger('id', 'password_hash', 'email', 'metadata');

-- --------------------------------------------------------------------
-- Encrypted credential lifecycle. encrypted_value is AES-256-GCM
-- ciphertext; redact it so the audit log records that the credential
-- changed (encrypted_value_changed boolean) without storing ciphertext.
-- --------------------------------------------------------------------
CREATE TRIGGER tr_audit_channel_credentials
    AFTER INSERT OR UPDATE OR DELETE ON channel_credentials
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id', 'encrypted_value');

-- --------------------------------------------------------------------
-- Access-control rules.
-- --------------------------------------------------------------------
-- category_group_visibility has a composite PK (category_id, group_id);
-- pk_text records category_id as the locator.
CREATE TRIGGER tr_audit_category_group_visibility
    AFTER INSERT OR UPDATE OR DELETE ON category_group_visibility
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('category_id');

CREATE TRIGGER tr_audit_documentation_page_visibility
    AFTER INSERT OR UPDATE OR DELETE ON documentation_page_visibility
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_documentation_collection_visibility
    AFTER INSERT OR UPDATE OR DELETE ON documentation_collection_visibility
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

-- --------------------------------------------------------------------
-- Automation config + agent templates + user prefs + plugin registry.
-- --------------------------------------------------------------------
CREATE TRIGGER tr_audit_assignment_rule_state
    AFTER INSERT OR UPDATE OR DELETE ON assignment_rule_state
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('rule_id');

CREATE TRIGGER tr_audit_canned_responses
    AFTER INSERT OR UPDATE OR DELETE ON canned_responses
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_notification_preferences
    AFTER INSERT OR UPDATE OR DELETE ON notification_preferences
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_plugin_registry_state
    AFTER INSERT OR UPDATE OR DELETE ON plugin_registry_state
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');
