-- Phase 3c.2 wave 2 — RLS for channels and email infrastructure.
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
-- Five tables covered: channels and their two satellite tables
-- (credentials, messages), canned response templates, and the
-- outbound email queue. The `nosdesk_app` role and its grants are
-- already in place from the tickets POC migration, so this
-- migration only touches the new tables.
--
-- Note: `outbound_emails` is also written by the background email
-- queue worker / scheduled sweeper in `services/email_queue/`,
-- which runs outside any HTTP request context. Those code paths
-- need to set `app.bypass_workspace_check = 'true'` via
-- `session::with_actor_bypass_context` (Phase 3g follow-up — they
-- are cross-workspace platform jobs); this migration enables the
-- policy and the follow-up wires the scheduler through the bypass.

ALTER TABLE channels ENABLE ROW LEVEL SECURITY;
ALTER TABLE channels FORCE ROW LEVEL SECURITY;
CREATE POLICY channels_workspace_isolation ON channels
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE channel_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE channel_credentials FORCE ROW LEVEL SECURITY;
CREATE POLICY channel_credentials_workspace_isolation ON channel_credentials
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE channel_messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE channel_messages FORCE ROW LEVEL SECURITY;
CREATE POLICY channel_messages_workspace_isolation ON channel_messages
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE canned_responses ENABLE ROW LEVEL SECURITY;
ALTER TABLE canned_responses FORCE ROW LEVEL SECURITY;
CREATE POLICY canned_responses_workspace_isolation ON canned_responses
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE outbound_emails ENABLE ROW LEVEL SECURITY;
ALTER TABLE outbound_emails FORCE ROW LEVEL SECURITY;
CREATE POLICY outbound_emails_workspace_isolation ON outbound_emails
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
