-- Phase 3c.2 wave 2 — RLS for the notifications + plugins aggregates.
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
-- Seven tables covered, parents first then dependents. The
-- `nosdesk_app` role and its grants are already in place from
-- the tickets POC migration, so this migration only touches the
-- new tables.
--
-- Note: the notification dispatch service (services/notifications/*)
-- and the notifications.rs handler still go through their own
-- pool checkouts without setting `app.workspace_id`. Phase 3g
-- moves the service onto the GUC-priming path; until then, the
-- runtime still connects as the migration role (not nosdesk_app)
-- and bypasses RLS entirely. The policies below are the
-- enforcement substrate for the role flip.

ALTER TABLE notifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE notifications FORCE ROW LEVEL SECURITY;
CREATE POLICY notifications_workspace_isolation ON notifications
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE notification_preferences ENABLE ROW LEVEL SECURITY;
ALTER TABLE notification_preferences FORCE ROW LEVEL SECURITY;
CREATE POLICY notification_preferences_workspace_isolation ON notification_preferences
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE plugins ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugins FORCE ROW LEVEL SECURITY;
CREATE POLICY plugins_workspace_isolation ON plugins
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE plugin_collection_schemas ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugin_collection_schemas FORCE ROW LEVEL SECURITY;
CREATE POLICY plugin_collection_schemas_workspace_isolation ON plugin_collection_schemas
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE plugin_collection_rows ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugin_collection_rows FORCE ROW LEVEL SECURITY;
CREATE POLICY plugin_collection_rows_workspace_isolation ON plugin_collection_rows
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE plugin_data ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugin_data FORCE ROW LEVEL SECURITY;
CREATE POLICY plugin_data_workspace_isolation ON plugin_data
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE plugin_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugin_activity FORCE ROW LEVEL SECURITY;
CREATE POLICY plugin_activity_workspace_isolation ON plugin_activity
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
