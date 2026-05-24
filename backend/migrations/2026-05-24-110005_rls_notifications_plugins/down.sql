-- Reverse 2026-05-24-110005_rls_notifications_plugins. Drop in
-- dependent-first order so the ordering mirrors `up.sql` walked
-- backwards.

DROP POLICY IF EXISTS plugin_activity_workspace_isolation ON plugin_activity;
ALTER TABLE plugin_activity NO FORCE ROW LEVEL SECURITY;
ALTER TABLE plugin_activity DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS plugin_data_workspace_isolation ON plugin_data;
ALTER TABLE plugin_data NO FORCE ROW LEVEL SECURITY;
ALTER TABLE plugin_data DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS plugin_collection_rows_workspace_isolation ON plugin_collection_rows;
ALTER TABLE plugin_collection_rows NO FORCE ROW LEVEL SECURITY;
ALTER TABLE plugin_collection_rows DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS plugin_collection_schemas_workspace_isolation ON plugin_collection_schemas;
ALTER TABLE plugin_collection_schemas NO FORCE ROW LEVEL SECURITY;
ALTER TABLE plugin_collection_schemas DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS plugins_workspace_isolation ON plugins;
ALTER TABLE plugins NO FORCE ROW LEVEL SECURITY;
ALTER TABLE plugins DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS notification_preferences_workspace_isolation ON notification_preferences;
ALTER TABLE notification_preferences NO FORCE ROW LEVEL SECURITY;
ALTER TABLE notification_preferences DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS notifications_workspace_isolation ON notifications;
ALTER TABLE notifications NO FORCE ROW LEVEL SECURITY;
ALTER TABLE notifications DISABLE ROW LEVEL SECURITY;
