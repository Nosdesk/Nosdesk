-- Phase 3a — RLS policy POC on the tickets table.
--
-- Single-table proof-point for the multi-tenant isolation strategy.
-- All other tenant tables get the same treatment in Phase 3c once
-- this validates end-to-end.
--
-- Strict policy by design: a forgotten GUC returns zero rows (an
-- obvious empty-result bug in staging) instead of silently
-- bypassing isolation. Cross-workspace ops opt in explicitly via
-- `app.bypass_workspace_check = 'true'` (set inside
-- `with_actor_bypass_context`).
--
-- Provisions the `nosdesk_app` role that the production app
-- connection should run as. Superusers and roles with the
-- BYPASSRLS attribute ignore policies even when FORCE RLS is set,
-- so a non-superuser, NOBYPASSRLS role is mandatory for isolation
-- to mean anything. The migration role (the connection diesel is
-- running migrations as) stays a superuser and keeps administering
-- the schema; the app role is just for runtime traffic. Switching
-- the app's DATABASE_URL to log in as `nosdesk_app` is a Phase 3c
-- ops change; for now tests SET LOCAL ROLE to it to exercise the
-- RLS path.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'nosdesk_app') THEN
        CREATE ROLE nosdesk_app NOLOGIN NOBYPASSRLS;
    END IF;
END
$$;

GRANT USAGE ON SCHEMA public TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO nosdesk_app;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO nosdesk_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO nosdesk_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO nosdesk_app;

ALTER TABLE tickets ENABLE ROW LEVEL SECURITY;
-- FORCE so table-owner connections don't bypass the policy if the
-- app ever ends up running as the owner (defence in depth — the
-- primary defence is `nosdesk_app` being a non-owner non-superuser).
ALTER TABLE tickets FORCE ROW LEVEL SECURITY;

CREATE POLICY tickets_workspace_isolation ON tickets
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
