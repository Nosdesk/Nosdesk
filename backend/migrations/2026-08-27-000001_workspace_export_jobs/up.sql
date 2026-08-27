-- Self-serve workspace data export: the Art 28(3)(g) "return" path a customer
-- uses to take their data before an account deletion erases it (account-erasure
-- Phase 3 prerequisite, on the control plane). One row per export request; the
-- artifact is a storage-backed ZIP (see services::workspace_export) and this row
-- tracks the job plus a bounded download window (expires_at, set from
-- completion). Owner-gated and self-serve, unlike the platform-admin export.
CREATE TABLE workspace_export_jobs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id integer NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    requested_by uuid,
    status varchar(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
    file_path text,
    file_size bigint,
    error_message text,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    -- Download window; NULL until completion, then completed_at + TTL. The
    -- retention sweep deletes the artifact + row once past this.
    expires_at timestamptz
);

CREATE INDEX idx_workspace_export_jobs_workspace
    ON workspace_export_jobs (workspace_id, created_at DESC);
-- Supports the "at most one active job per workspace" + "one completed per day"
-- rate-limit checks.
CREATE INDEX idx_workspace_export_jobs_active
    ON workspace_export_jobs (workspace_id) WHERE status IN ('pending', 'processing');
CREATE INDEX idx_workspace_export_jobs_expiry
    ON workspace_export_jobs (expires_at) WHERE expires_at IS NOT NULL;

-- New-table ownership + runtime grants. A new table must set these explicitly
-- (see workspace_notification_defaults): nosdesk_admin must OWN it so the
-- BYPASSRLS export/background role sees it via information_schema and can write
-- the job row from the background task; nosdesk_app needs DML for the RLS-scoped
-- tenant reads (enqueue / status / download).
ALTER TABLE public.workspace_export_jobs OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_export_jobs TO nosdesk_app;

-- Workspace isolation for the RLS-enforced runtime role (mirrors
-- workspace_notification_defaults). Background writes run under the BYPASSRLS
-- role and are unaffected.
ALTER TABLE workspace_export_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_export_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_export_jobs_workspace_isolation
    ON workspace_export_jobs
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
