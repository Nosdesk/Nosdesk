-- Add the `plugin` aggregate so plugin lifecycle events
-- (install / update / disable / quarantine / uninstall) can flow
-- through sync_actions as tier-1 typed events. Plugin local storage
-- (plugin_data, plugin_collection_*) stays audit-only via the
-- audit_log trigger.
--
-- Postgres 12+ allows ALTER TYPE ... ADD VALUE inside a transaction
-- block, with the single restriction that the new value cannot be
-- USED in the same transaction. This migration only ADDs the value;
-- the first row to use it ('plugin') comes from a later transaction
-- (the next plugin install). compose.yaml pins postgres:18, so the
-- restriction is safely below our minimum supported version.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'plugin';
