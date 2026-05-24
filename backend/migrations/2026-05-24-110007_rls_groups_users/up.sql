-- Phase 3c.2 wave 2 — RLS for groups, group memberships, user_ticket_views,
-- and site_settings.
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
-- Five tables covered, in dependency-friendly order: parent
-- tables first, join tables last. The `nosdesk_app` role and its
-- grants are already in place from the tickets POC migration, so
-- this migration only touches the new tables.
--
-- Known follow-up (NOT fixed here): `user_ticket_views` is
-- accessed via `UserTicketViewsRepository` which wraps a pool
-- and acquires its own connection without going through
-- `TenantConn` / `set_actor`. Once RLS is on, those callers will
-- return zero rows until that repository is refactored to take
-- a tenant-scoped connection (Phase 3g-class follow-up).

ALTER TABLE groups ENABLE ROW LEVEL SECURITY;
ALTER TABLE groups FORCE ROW LEVEL SECURITY;
CREATE POLICY groups_workspace_isolation ON groups
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE group_includes ENABLE ROW LEVEL SECURITY;
ALTER TABLE group_includes FORCE ROW LEVEL SECURITY;
CREATE POLICY group_includes_workspace_isolation ON group_includes
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE user_groups ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_groups FORCE ROW LEVEL SECURITY;
CREATE POLICY user_groups_workspace_isolation ON user_groups
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE user_ticket_views ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_ticket_views FORCE ROW LEVEL SECURITY;
CREATE POLICY user_ticket_views_workspace_isolation ON user_ticket_views
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE site_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE site_settings FORCE ROW LEVEL SECURITY;
CREATE POLICY site_settings_workspace_isolation ON site_settings
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
