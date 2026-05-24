DROP POLICY IF EXISTS ticket_assets_workspace_isolation ON ticket_assets;
ALTER TABLE ticket_assets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE ticket_assets DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS asset_usage_log_workspace_isolation ON asset_usage_log;
ALTER TABLE asset_usage_log NO FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_usage_log DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS asset_audits_workspace_isolation ON asset_audits;
ALTER TABLE asset_audits NO FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_audits DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS asset_groups_workspace_isolation ON asset_groups;
ALTER TABLE asset_groups NO FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_groups DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS asset_kinds_workspace_isolation ON asset_kinds;
ALTER TABLE asset_kinds NO FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_kinds DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS assets_workspace_isolation ON assets;
ALTER TABLE assets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE assets DISABLE ROW LEVEL SECURITY;
