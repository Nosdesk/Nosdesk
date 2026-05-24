DROP POLICY IF EXISTS site_settings_workspace_isolation ON site_settings;
ALTER TABLE site_settings NO FORCE ROW LEVEL SECURITY;
ALTER TABLE site_settings DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS user_ticket_views_workspace_isolation ON user_ticket_views;
ALTER TABLE user_ticket_views NO FORCE ROW LEVEL SECURITY;
ALTER TABLE user_ticket_views DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS user_groups_workspace_isolation ON user_groups;
ALTER TABLE user_groups NO FORCE ROW LEVEL SECURITY;
ALTER TABLE user_groups DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS group_includes_workspace_isolation ON group_includes;
ALTER TABLE group_includes NO FORCE ROW LEVEL SECURITY;
ALTER TABLE group_includes DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS groups_workspace_isolation ON groups;
ALTER TABLE groups NO FORCE ROW LEVEL SECURITY;
ALTER TABLE groups DISABLE ROW LEVEL SECURITY;
