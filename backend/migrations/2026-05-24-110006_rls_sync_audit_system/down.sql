DROP POLICY IF EXISTS api_tokens_workspace_isolation ON api_tokens;
ALTER TABLE api_tokens NO FORCE ROW LEVEL SECURITY;
ALTER TABLE api_tokens DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS backup_jobs_workspace_isolation ON backup_jobs;
ALTER TABLE backup_jobs NO FORCE ROW LEVEL SECURITY;
ALTER TABLE backup_jobs DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS search_query_log_workspace_isolation ON search_query_log;
ALTER TABLE search_query_log NO FORCE ROW LEVEL SECURITY;
ALTER TABLE search_query_log DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS csp_reports_workspace_isolation ON csp_reports;
ALTER TABLE csp_reports NO FORCE ROW LEVEL SECURITY;
ALTER TABLE csp_reports DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS sync_delta_tokens_workspace_isolation ON sync_delta_tokens;
ALTER TABLE sync_delta_tokens NO FORCE ROW LEVEL SECURITY;
ALTER TABLE sync_delta_tokens DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS sync_history_workspace_isolation ON sync_history;
ALTER TABLE sync_history NO FORCE ROW LEVEL SECURITY;
ALTER TABLE sync_history DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS sync_actions_workspace_isolation ON sync_actions;
ALTER TABLE sync_actions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE sync_actions DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS audit_log_workspace_isolation ON audit_log;
ALTER TABLE audit_log NO FORCE ROW LEVEL SECURITY;
ALTER TABLE audit_log DISABLE ROW LEVEL SECURITY;
