-- Phase 3c.2 — RLS rollout for the documentation aggregate.
--
-- Same shape as 2026-05-24-100000_tickets_rls_poc/up.sql: enable +
-- force RLS on each table, then attach a workspace-isolation policy
-- whose USING and WITH CHECK clauses both compare `workspace_id` to
-- the per-transaction `app.workspace_id` GUC. Cross-workspace work
-- opts in via `app.bypass_workspace_check = 'true'` (set inside
-- `with_actor_bypass_context` / `TenantConn::unscoped_run`).
--
-- The `nosdesk_app` role and its grants were provisioned in the
-- tickets POC migration; this migration only adds policies. The
-- existing `ALTER DEFAULT PRIVILEGES ... GRANT ... TO nosdesk_app`
-- from that earlier migration already covers tables created after
-- it, but these tables predate the POC so they need no extra grant
-- here either — the POC's blanket `GRANT ... ON ALL TABLES IN SCHEMA
-- public TO nosdesk_app` already covered them.
--
-- One follow-up flagged for the main agent: the Yjs WebSocket path
-- (handlers/collaboration.rs) writes to `article_contents`,
-- `documentation_revisions`, and (transitively, via the persistence
-- helpers) `documentation_pages` outside the normal HTTP handler
-- chain. Those writes don't go through TenantConn, so this RLS
-- migration will start failing them unless that handler is bridged
-- to a `with_actor_context`-style wrapper. See the agent report for
-- detail.

ALTER TABLE documentation_pages ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_pages FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_pages_workspace_isolation ON documentation_pages
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_collections ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_collections FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_collections_workspace_isolation ON documentation_collections
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_collection_pages ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_collection_pages FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_collection_pages_workspace_isolation ON documentation_collection_pages
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_collection_visibility ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_collection_visibility FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_collection_visibility_workspace_isolation ON documentation_collection_visibility
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_page_embeddings ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_page_embeddings FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_page_embeddings_workspace_isolation ON documentation_page_embeddings
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_page_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_page_tickets FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_page_tickets_workspace_isolation ON documentation_page_tickets
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_page_visibility ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_page_visibility FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_page_visibility_workspace_isolation ON documentation_page_visibility
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_revisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_revisions FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_revisions_workspace_isolation ON documentation_revisions
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_starred_pages ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_starred_pages FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_starred_pages_workspace_isolation ON documentation_starred_pages
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE documentation_subscriptions ENABLE ROW LEVEL SECURITY;
ALTER TABLE documentation_subscriptions FORCE ROW LEVEL SECURITY;
CREATE POLICY documentation_subscriptions_workspace_isolation ON documentation_subscriptions
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE article_contents ENABLE ROW LEVEL SECURITY;
ALTER TABLE article_contents FORCE ROW LEVEL SECURITY;
CREATE POLICY article_contents_workspace_isolation ON article_contents
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE article_content_revisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE article_content_revisions FORCE ROW LEVEL SECURITY;
CREATE POLICY article_content_revisions_workspace_isolation ON article_content_revisions
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
