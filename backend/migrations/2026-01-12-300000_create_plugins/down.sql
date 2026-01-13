-- Drop indexes
DROP INDEX IF EXISTS idx_plugin_activity_created_at;
DROP INDEX IF EXISTS idx_plugin_activity_plugin_id;
DROP INDEX IF EXISTS idx_plugin_storage_key;
DROP INDEX IF EXISTS idx_plugin_storage_plugin_id;
DROP INDEX IF EXISTS idx_plugin_settings_plugin_id;
DROP INDEX IF EXISTS idx_plugins_name;
DROP INDEX IF EXISTS idx_plugins_enabled;

-- Drop tables (in order of dependencies)
DROP TABLE IF EXISTS plugin_activity;
DROP TABLE IF EXISTS plugin_storage;
DROP TABLE IF EXISTS plugin_settings;
DROP TABLE IF EXISTS plugins;
