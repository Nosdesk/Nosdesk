-- User-submitted bug reports from the in-app "Report a problem"
-- modal. Captured manually by the user, never auto-generated. One
-- row per submission. Carries a small breadcrumb trail of the
-- user's recent route changes and API calls so an admin reading the
-- report can reconstruct what the user was doing just before.
--
-- Workspace-scoped, RLS-isolated, owned by nosdesk_admin. Not
-- deduplicated (each submission is a deliberate user action), not
-- partitioned (volume is bounded by user attention), and not
-- forwarded externally in the OSS build.
--
-- The wider client diagnostics pipeline (auto-captured error /
-- unhandled_rejection / api_failure events) lives separately and
-- lands in a follow-up. Keeping bug reports as their own product
-- surface from day one means the table name stays meaningful when
-- the diagnostics table arrives.

CREATE TABLE bug_reports (
    id              BIGSERIAL PRIMARY KEY,
    workspace_id    INTEGER NOT NULL
        REFERENCES workspaces(id) ON DELETE CASCADE
        DEFAULT NULLIF(current_setting('app.workspace_id', true), '')::int,
    user_uuid       UUID REFERENCES users(uuid) ON DELETE SET NULL,

    -- Per-tab UUIDv4 minted client-side and stored in sessionStorage.
    -- Same value rides on the X-Nosdesk-Trace-Id request header so
    -- backend tracing spans correlate by browser session.
    session_id      UUID NOT NULL,

    -- Free text from the user. Capped to 4 KiB at the handler to
    -- keep rows bounded; the database CHECK is defence in depth.
    description     TEXT NOT NULL,

    -- Page the user was on when they clicked the button.
    -- Server-side stripped of query string and fragment before
    -- insert.
    url             TEXT NOT NULL,

    -- Ring buffer snapshot at submission time. Array of objects of
    -- the form { category: "route" | "api", ts: <epoch_ms>, data:
    -- { ... } }, capped at 10 entries. Shape validated at the
    -- handler.
    breadcrumbs     JSONB NOT NULL DEFAULT '[]'::jsonb,

    -- Bundle SHA the client was running. Comes from
    -- VITE_BUILD_SHA injected at vite build time. "dev" in dev
    -- mode.
    build_sha       VARCHAR(64) NOT NULL,

    -- Trimmed and tag-char-stripped at the handler.
    user_agent      TEXT,

    -- { "w": <int>, "h": <int> }; useful when reproducing layout
    -- bugs.
    viewport        JSONB,

    occurred_at     TIMESTAMPTZ NOT NULL,
    received_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT bug_reports_description_size CHECK (octet_length(description) < 4096),
    CONSTRAINT bug_reports_breadcrumbs_size CHECK (octet_length(breadcrumbs::text) < 16384)
);

CREATE INDEX bug_reports_workspace_occurred_idx
    ON bug_reports (workspace_id, occurred_at DESC);
CREATE INDEX bug_reports_session_idx
    ON bug_reports (session_id);
CREATE INDEX bug_reports_user_occurred_idx
    ON bug_reports (user_uuid, occurred_at DESC)
    WHERE user_uuid IS NOT NULL;

ALTER TABLE bug_reports ENABLE ROW LEVEL SECURITY;
ALTER TABLE bug_reports FORCE ROW LEVEL SECURITY;
CREATE POLICY bug_reports_workspace_isolation ON bug_reports
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

-- The bulk re-owner loop in 2026-06-03-040000_tenant_table_ownership
-- only ran for tables existing at that migration's time. New tables
-- must self-own.
ALTER TABLE bug_reports OWNER TO nosdesk_admin;
ALTER SEQUENCE bug_reports_id_seq OWNER TO nosdesk_admin;
