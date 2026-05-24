DROP POLICY IF EXISTS import_jobs_workspace_isolation ON import_jobs;
ALTER TABLE import_jobs NO FORCE ROW LEVEL SECURITY;
ALTER TABLE import_jobs DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS assignment_log_workspace_isolation ON assignment_log;
ALTER TABLE assignment_log NO FORCE ROW LEVEL SECURITY;
ALTER TABLE assignment_log DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS assignment_rule_state_workspace_isolation ON assignment_rule_state;
ALTER TABLE assignment_rule_state NO FORCE ROW LEVEL SECURITY;
ALTER TABLE assignment_rule_state DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS assignment_rules_workspace_isolation ON assignment_rules;
ALTER TABLE assignment_rules NO FORCE ROW LEVEL SECURITY;
ALTER TABLE assignment_rules DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS webhook_deliveries_workspace_isolation ON webhook_deliveries;
ALTER TABLE webhook_deliveries NO FORCE ROW LEVEL SECURITY;
ALTER TABLE webhook_deliveries DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS webhooks_workspace_isolation ON webhooks;
ALTER TABLE webhooks NO FORCE ROW LEVEL SECURITY;
ALTER TABLE webhooks DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS working_calendar_holidays_workspace_isolation ON working_calendar_holidays;
ALTER TABLE working_calendar_holidays NO FORCE ROW LEVEL SECURITY;
ALTER TABLE working_calendar_holidays DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS working_calendars_workspace_isolation ON working_calendars;
ALTER TABLE working_calendars NO FORCE ROW LEVEL SECURITY;
ALTER TABLE working_calendars DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS sla_policies_workspace_isolation ON sla_policies;
ALTER TABLE sla_policies NO FORCE ROW LEVEL SECURITY;
ALTER TABLE sla_policies DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS saved_views_workspace_isolation ON saved_views;
ALTER TABLE saved_views NO FORCE ROW LEVEL SECURITY;
ALTER TABLE saved_views DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS knowledge_gap_signals_workspace_isolation ON knowledge_gap_signals;
ALTER TABLE knowledge_gap_signals NO FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_gap_signals DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS knowledge_gaps_workspace_isolation ON knowledge_gaps;
ALTER TABLE knowledge_gaps NO FORCE ROW LEVEL SECURITY;
ALTER TABLE knowledge_gaps DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS cycle_tickets_workspace_isolation ON cycle_tickets;
ALTER TABLE cycle_tickets NO FORCE ROW LEVEL SECURITY;
ALTER TABLE cycle_tickets DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS cycles_workspace_isolation ON cycles;
ALTER TABLE cycles NO FORCE ROW LEVEL SECURITY;
ALTER TABLE cycles DISABLE ROW LEVEL SECURITY;
