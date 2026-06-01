-- =====================================================================
-- Yjs document snapshots — forward-compatibility schema only.
--
-- Per M5 product-side handoff Task 8 + senior peer review #4: Yjs
-- uncommitted operations live in actor memory hydrated from Redis
-- pub/sub. When a backend machine restarts (deploy or crash),
-- in-flight collaborative edits are lost. The full fix is a debounced
-- snapshot worker that persists doc state to Postgres every 5s of
-- inactivity.
--
-- This migration is the table-shape part of that fix; the worker
-- itself is Phase 1.5 (post-M5). Landing the schema now means the
-- future worker is a code-only PR — no schema-migration coordination
-- between hosted + self-hosted operators.
--
-- Per-workspace tenant isolation via RLS, same pattern as every
-- other tenant table (see `2026-05-24-110001_rls_projects_workflow`
-- for the reference): ENABLE + FORCE + WITH CHECK policy pinned to
-- the `app.workspace_id` GUC.
-- =====================================================================

CREATE TABLE yjs_snapshots (
    id           BIGSERIAL PRIMARY KEY,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    document_id  TEXT NOT NULL,
    -- Yjs Y.encodeStateAsUpdate output. Restoring a doc means
    -- decoding this back into the in-memory Awareness.
    snapshot     BYTEA NOT NULL,
    -- Yjs Y.encodeStateVector output. Reserved for the future
    -- delta-encoding worker; the first cut just persists the full
    -- snapshot and ignores this.
    state_vector BYTEA NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The worker's hot-path query is "latest snapshot for (workspace,
-- document)". DESC on created_at lets it pop the newest row with a
-- single index scan; the (workspace_id, document_id) prefix carries
-- tenant isolation cheaply.
CREATE INDEX yjs_snapshots_lookup_idx
    ON yjs_snapshots (workspace_id, document_id, created_at DESC);

-- Tenant isolation matches every other tenant table.
ALTER TABLE yjs_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE yjs_snapshots FORCE ROW LEVEL SECURITY;

CREATE POLICY yjs_snapshots_tenant_isolation ON yjs_snapshots
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
    );

GRANT SELECT, INSERT, UPDATE, DELETE ON yjs_snapshots TO nosdesk_app;
GRANT USAGE ON SEQUENCE yjs_snapshots_id_seq TO nosdesk_app;
