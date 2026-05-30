-- Per-use log for canned responses. One row per insertion from the
-- ticket composer; the admin list page rolls these up into the
-- "Inserts (30d)" column so admins can spot underperforming
-- templates and retire them. Mirrors the converged industry metric
-- (Freshdesk "uses" report, Intercom "Insertions in last 30 days").
--
-- Workspace-local: rows stay inside the operator's own database and
-- are RLS-scoped to the workspace. No external transmission, no
-- phone-home, no aggregation across tenants. The counter is for the
-- admin's own visibility into their own library.
--
-- Append-only: rows are never updated. Insertions are kept for the
-- lifetime of the parent canned response and cascade on delete so
-- we don't leak orphan rows when an admin retires a template.
--
-- Deliberately not wired into the audit trigger family (see
-- 2026-05-25-140000_attach_audit_triggers_v2). Insertion logging
-- is high-frequency, low-sensitivity; audit_log churn isn't
-- justified.

CREATE TABLE canned_response_insertions (
    id BIGSERIAL PRIMARY KEY,
    canned_response_id INTEGER NOT NULL
        REFERENCES canned_responses(id) ON DELETE CASCADE,
    -- Nullable so a user delete preserves the count rather than
    -- vaporising history; same rationale as canned_responses.created_by.
    user_uuid UUID
        REFERENCES users(uuid) ON DELETE SET NULL,
    -- Nullable because insertions may originate from contexts that
    -- aren't bound to a ticket yet (a future "preview" surface), and
    -- because a ticket deletion shouldn't drop the counter.
    ticket_id INTEGER
        REFERENCES tickets(id) ON DELETE SET NULL,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    workspace_id INTEGER NOT NULL
        REFERENCES workspaces(id) ON DELETE CASCADE
);

-- Drives the 30-day rollup on the admin list page. The composite
-- order (canned_response_id, inserted_at DESC) lets Postgres
-- short-circuit the scan once it walks past the 30-day cutoff.
CREATE INDEX canned_response_insertions_response_time_idx
    ON canned_response_insertions (canned_response_id, inserted_at DESC);

-- Mirrors canned_responses isolation: workspace context comes from
-- app.workspace_id (set by the tenant connection wrapper) with a
-- bypass for service-internal callers.
ALTER TABLE canned_response_insertions ENABLE ROW LEVEL SECURITY;
ALTER TABLE canned_response_insertions FORCE ROW LEVEL SECURITY;
CREATE POLICY canned_response_insertions_workspace_isolation
    ON canned_response_insertions
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );
