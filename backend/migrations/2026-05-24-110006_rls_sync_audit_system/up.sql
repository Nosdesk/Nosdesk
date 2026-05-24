-- Phase 3c.2 (wave 2) — RLS rollout for the sync + audit + system
-- infrastructure tables.
--
-- Same strict workspace-isolation pattern as the tickets POC
-- (see 2026-05-24-100000_tickets_rls_poc): ENABLE + FORCE row level
-- security, then a policy that pins reads and writes to the current
-- `app.workspace_id` GUC, with an explicit bypass disjunct for
-- cross-workspace ops that set `app.bypass_workspace_check = 'true'`.
-- A forgotten GUC returns zero rows (surfaces as an obvious empty-
-- result bug in staging) rather than silently leaking across tenants.
--
-- Eight tables covered: two partitioned (audit_log, sync_actions),
-- six regular. The `nosdesk_app` role and its grants were
-- provisioned in the tickets POC migration; this migration only adds
-- policies.
--
-- Cross-cutting follow-ups flagged to the main agent:
--
--   1. middleware/api_token.rs queries `api_tokens` before the
--      workspace middleware sets any GUC. Token lookup must move to
--      `with_actor_bypass_context` (or equivalent) or token-based
--      auth breaks once this migration lands. Phase 3g-style fix.
--
--   2. csp_reports::report_violation is a public unauthenticated
--      endpoint; without a request actor the insert path needs a
--      bypass wrapper. Phase 3g.
--
--   3. handlers/search.rs spawns a background task that writes
--      search_query_log via a raw pool connection (no workspace GUC).
--      Phase 3g.
--
--   4. backup/restore background tokio::spawn writes to backup_jobs
--      via a raw pool connection (no workspace GUC). Phase 3g.
--
--   5. handlers/msgraph_integration.rs reads/writes sync_history and
--      sync_delta_tokens via raw pool connections from background
--      sync routines (out of wave-2 handler scope). Phase 3g.
--
--   6. handlers/sync/push.rs builds an ActorContext literal with
--      `workspace_id: None`. emit::record writes sync_actions inside
--      that actor, so post-RLS those writes need the GUC pinned.
--      Fix lives in push.rs.

-- Partitioned: audit_log. Postgres >=10 propagates the parent
-- policy to every existing and future partition (audit_log_default
-- and any monthly rotations), so attaching the policy here is
-- enough — partitions inherit the USING/WITH CHECK predicates from
-- the parent at query-planning time.
ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_log FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_log_workspace_isolation ON audit_log
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

-- Partitioned: sync_actions. Same parent-policy-applies-to-all-
-- partitions semantic as audit_log above. sync_actions_default and
-- any future partitions inherit the USING/WITH CHECK clauses
-- automatically (Postgres >=10).
ALTER TABLE sync_actions ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_actions FORCE ROW LEVEL SECURITY;
CREATE POLICY sync_actions_workspace_isolation ON sync_actions
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE sync_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_history FORCE ROW LEVEL SECURITY;
CREATE POLICY sync_history_workspace_isolation ON sync_history
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE sync_delta_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_delta_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY sync_delta_tokens_workspace_isolation ON sync_delta_tokens
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE csp_reports ENABLE ROW LEVEL SECURITY;
ALTER TABLE csp_reports FORCE ROW LEVEL SECURITY;
CREATE POLICY csp_reports_workspace_isolation ON csp_reports
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE search_query_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE search_query_log FORCE ROW LEVEL SECURITY;
CREATE POLICY search_query_log_workspace_isolation ON search_query_log
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE backup_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE backup_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY backup_jobs_workspace_isolation ON backup_jobs
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE api_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY api_tokens_workspace_isolation ON api_tokens
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
