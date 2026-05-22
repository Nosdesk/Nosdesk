-- CSV import audit + state machine. Two-phase imports: upload
-- + parse → dry-run → commit. The row tracks both phases plus
-- the file path so the admin UI can resurface in-progress jobs
-- and the audit trail survives the originating session.
--
-- Statuses:
--   parsed         file is on disk, dry-run hasn't run yet
--   dry_run_done   validation finished, summary populated, awaiting commit
--   committing     commit started
--   done           commit finished (records_committed populated)
--   failed         parse or commit failed; error_message populated
--
-- One job per uploaded file. Re-uploading the same file is a
-- new row. Files older than the retention window get cleaned up
-- by an admin sweep (not in this migration).

CREATE TABLE import_jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_type        VARCHAR(32) NOT NULL
        CHECK (job_type IN ('assets', 'users', 'tickets')),
    status          VARCHAR(32) NOT NULL DEFAULT 'parsed'
        CHECK (status IN ('parsed', 'dry_run_done', 'committing', 'done', 'failed')),
    filename        VARCHAR(255) NOT NULL,
    file_path       TEXT NOT NULL,
    created_by      UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ,

    -- Dry-run output: { "row_count": N, "would_create": M,
    --                   "would_update": K, "errors": [...] }
    -- The errors array carries up to a cap of per-row failures
    -- so the UI can render them without a separate fetch.
    summary         JSONB,

    -- Filled when status = 'done' or 'failed'.
    records_committed INT,
    error_message     TEXT
);

CREATE INDEX idx_import_jobs_created_by
    ON import_jobs (created_by, created_at DESC);

CREATE INDEX idx_import_jobs_status
    ON import_jobs (status) WHERE status IN ('parsed', 'dry_run_done', 'committing');
