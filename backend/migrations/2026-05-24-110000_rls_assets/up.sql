-- Phase 3c.2 — RLS for the assets aggregate.
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
-- Six tables covered, in dependency-friendly order: parent tables
-- first, join tables last. The `nosdesk_app` role and its grants
-- are already in place from the tickets POC migration, so this
-- migration only touches the new tables.

ALTER TABLE assets ENABLE ROW LEVEL SECURITY;
ALTER TABLE assets FORCE ROW LEVEL SECURITY;
CREATE POLICY assets_workspace_isolation ON assets
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE asset_kinds ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_kinds FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_kinds_workspace_isolation ON asset_kinds
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE asset_groups ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_groups FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_groups_workspace_isolation ON asset_groups
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE asset_audits ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_audits FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_audits_workspace_isolation ON asset_audits
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE asset_usage_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_usage_log FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_usage_log_workspace_isolation ON asset_usage_log
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE ticket_assets ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_assets FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_assets_workspace_isolation ON ticket_assets
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
