-- Phase 3c.2 wave 2 — RLS for the misc tenant tables.
--
-- Same strict workspace-isolation pattern as the tickets POC
-- (see 2026-05-24-100000_tickets_rls_poc): ENABLE + FORCE row
-- level security, then a policy that pins reads and writes to
-- the current `app.workspace_id` GUC, with an explicit bypass
-- disjunct for cross-workspace ops that set
-- `app.bypass_workspace_check = 'true'`. A forgotten GUC returns
-- zero rows (surfaces as an obvious empty-result bug in staging)
-- rather than silently leaking across tenants.
--
-- Fourteen tables covered: cycles + cycle_tickets (iteration
-- planning), knowledge_gaps + signals (gap analyser), saved_views
-- (per-user list-view filters), sla_policies + working_calendars +
-- working_calendar_holidays (SLA + business hours), webhooks +
-- webhook_deliveries (outbound HTTP), the AssignmentEngine trio
-- (assignment_rules + assignment_rule_state + assignment_log),
-- and import_jobs (bulk CSV import orchestration). The
-- `nosdesk_app` role and its grants are already in place from
-- the tickets POC migration; this migration only touches the new
-- tables.

ALTER TABLE cycles ENABLE ROW LEVEL SECURITY;
ALTER TABLE cycles FORCE ROW LEVEL SECURITY;
CREATE POLICY cycles_workspace_isolation ON cycles
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE cycle_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE cycle_tickets FORCE ROW LEVEL SECURITY;
CREATE POLICY cycle_tickets_workspace_isolation ON cycle_tickets
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE knowledge_gaps ENABLE ROW LEVEL SECURITY;
ALTER TABLE knowledge_gaps FORCE ROW LEVEL SECURITY;
CREATE POLICY knowledge_gaps_workspace_isolation ON knowledge_gaps
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE knowledge_gap_signals ENABLE ROW LEVEL SECURITY;
ALTER TABLE knowledge_gap_signals FORCE ROW LEVEL SECURITY;
CREATE POLICY knowledge_gap_signals_workspace_isolation ON knowledge_gap_signals
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE saved_views ENABLE ROW LEVEL SECURITY;
ALTER TABLE saved_views FORCE ROW LEVEL SECURITY;
CREATE POLICY saved_views_workspace_isolation ON saved_views
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE sla_policies ENABLE ROW LEVEL SECURITY;
ALTER TABLE sla_policies FORCE ROW LEVEL SECURITY;
CREATE POLICY sla_policies_workspace_isolation ON sla_policies
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE working_calendars ENABLE ROW LEVEL SECURITY;
ALTER TABLE working_calendars FORCE ROW LEVEL SECURITY;
CREATE POLICY working_calendars_workspace_isolation ON working_calendars
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE working_calendar_holidays ENABLE ROW LEVEL SECURITY;
ALTER TABLE working_calendar_holidays FORCE ROW LEVEL SECURITY;
CREATE POLICY working_calendar_holidays_workspace_isolation ON working_calendar_holidays
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE webhooks ENABLE ROW LEVEL SECURITY;
ALTER TABLE webhooks FORCE ROW LEVEL SECURITY;
CREATE POLICY webhooks_workspace_isolation ON webhooks
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE webhook_deliveries ENABLE ROW LEVEL SECURITY;
ALTER TABLE webhook_deliveries FORCE ROW LEVEL SECURITY;
CREATE POLICY webhook_deliveries_workspace_isolation ON webhook_deliveries
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE assignment_rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE assignment_rules FORCE ROW LEVEL SECURITY;
CREATE POLICY assignment_rules_workspace_isolation ON assignment_rules
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE assignment_rule_state ENABLE ROW LEVEL SECURITY;
ALTER TABLE assignment_rule_state FORCE ROW LEVEL SECURITY;
CREATE POLICY assignment_rule_state_workspace_isolation ON assignment_rule_state
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE assignment_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE assignment_log FORCE ROW LEVEL SECURITY;
CREATE POLICY assignment_log_workspace_isolation ON assignment_log
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE import_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE import_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY import_jobs_workspace_isolation ON import_jobs
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
