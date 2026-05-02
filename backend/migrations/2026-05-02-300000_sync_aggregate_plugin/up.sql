-- Add the `plugin` aggregate so plugin lifecycle events
-- (install / update / disable / quarantine / uninstall) can flow
-- through sync_actions as tier-1 typed events. Plugin local storage
-- (plugin_data, plugin_collection_*) stays audit-only via the
-- audit_log trigger.
--
-- ALTER TYPE ... ADD VALUE is non-transactional in Postgres, so this
-- runs outside the migration's enclosing transaction. Diesel handles
-- that via the per-statement isolation it does for sql_query.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'plugin';
