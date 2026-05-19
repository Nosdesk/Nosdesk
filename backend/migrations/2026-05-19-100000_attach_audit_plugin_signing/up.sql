-- Attach audit_log_trigger to the plugin trust-chain tables.
--
-- Plugin install/update/uninstall, publisher revocation, and local
-- signing-key rotation are all forensically important: a customer
-- security review wants to answer "who installed this plugin, who
-- revoked that publisher key, when did the local signing key
-- change?" and the trigger gives us before/after JSON for free.
--
-- plugin_data and plugin_collection_rows were already attached in
-- 2026-05-02-200000_attach_audit_triggers (tier-3 forensics on
-- per-plugin settings + storage). Tier-1 entities followed in
-- 2026-05-11-210000_attach_audit_tier1. This migration closes the
-- last gap: the rows that anchor the trust chain itself.

-- Note on row size: the generic audit_log_trigger to_jsonb()s the
-- entire row, so each plugins INSERT/UPDATE includes the
-- bundle_js BYTEA (up to ~1 MiB) as base64 in audit_log.after_jsonb.
-- Plugin writes are rare (install / enable / bundle refresh) so
-- the absolute cost is small, but if this ever grows we can swap
-- in a per-table trigger function that strips bundle_js + icon_svg
-- before to_jsonb(). The audit_log partition retention pruning
-- (W6d) bounds long-term growth.
CREATE TRIGGER tr_audit_plugins
    AFTER INSERT OR UPDATE OR DELETE ON plugins
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('uuid');

-- plugin_trusted_publishers gets a per-op split because the
-- registry sync runs every 24h and ON CONFLICT DO UPDATE always
-- writes, even when the new row equals the existing one. Without
-- the WHEN guard on UPDATE we'd accumulate one no-op audit row
-- per publisher per day. INSERT (new publisher added) and DELETE
-- (publisher removed) always matter, so they fire unconditionally.
CREATE TRIGGER tr_audit_plugin_trusted_publishers_ins
    AFTER INSERT ON plugin_trusted_publishers
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_plugin_trusted_publishers_upd
    AFTER UPDATE ON plugin_trusted_publishers
    FOR EACH ROW
    WHEN (OLD IS DISTINCT FROM NEW)
    EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_plugin_trusted_publishers_del
    AFTER DELETE ON plugin_trusted_publishers
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

-- Single-row table (CHECK id = 1) but still worth auditing: the
-- pubkey/encrypted_sk pair changing represents a key rotation
-- event and the audit row preserves the prior fingerprint for
-- after-the-fact verification of plugin installs signed before
-- the rotation.
CREATE TRIGGER tr_audit_plugin_local_signing_key
    AFTER INSERT OR UPDATE OR DELETE ON plugin_local_signing_key
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');
