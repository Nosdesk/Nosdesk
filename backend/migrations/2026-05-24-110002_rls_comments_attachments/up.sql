-- Phase 3c.2 — RLS rollout to ticket-child tables.
--
-- Extends the workspace-isolation pattern proven on `tickets`
-- (see 2026-05-24-100000_tickets_rls_poc) to the four tables that
-- share a ticket's tenancy: comments, attachments, linked_tickets,
-- ticket_watchers. Same strict shape: a forgotten GUC returns zero
-- rows instead of silently widening visibility, and cross-workspace
-- ops opt in explicitly via `app.bypass_workspace_check`.
--
-- The `nosdesk_app` role and its grants are already provisioned by
-- the POC migration; this migration only manipulates table-level RLS.

ALTER TABLE comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE comments FORCE ROW LEVEL SECURITY;

CREATE POLICY comments_workspace_isolation ON comments
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE attachments ENABLE ROW LEVEL SECURITY;
ALTER TABLE attachments FORCE ROW LEVEL SECURITY;

CREATE POLICY attachments_workspace_isolation ON attachments
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE linked_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE linked_tickets FORCE ROW LEVEL SECURITY;

CREATE POLICY linked_tickets_workspace_isolation ON linked_tickets
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE ticket_watchers ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_watchers FORCE ROW LEVEL SECURITY;

CREATE POLICY ticket_watchers_workspace_isolation ON ticket_watchers
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
