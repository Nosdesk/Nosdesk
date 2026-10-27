-- A workspace export imported into an existing workspace runs as a job: the
-- archive is uploaded and parked under `workspace-imports/{id}.nosdesk` in the
-- workspace's storage, checked in the background (version, untouched, plan)
-- into a preview, then imported in the background once the owner confirms.
-- The job keeps the report and the people the import created, so a reset can
-- remove them again. Separate from `import_jobs` (CSV and documentation
-- imports): this job is CP-visible, runs on the owner role and must survive
-- its own clear step.
CREATE TABLE public.workspace_import_jobs (
    id              uuid        PRIMARY KEY DEFAULT uuidv7(),
    workspace_id    integer     NOT NULL
                    DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                    REFERENCES public.workspaces(id) ON DELETE CASCADE,
    -- Who started it: `owner` (the tenant route), `operator` (platform
    -- admin), `control_plane` (hosted, through the internal API).
    runner          varchar(16) NOT NULL
                    CHECK (runner IN ('owner', 'operator', 'control_plane')),
    requested_by    uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    state           varchar(16) NOT NULL DEFAULT 'uploaded'
                    CHECK (state IN ('uploaded', 'checking', 'previewed', 'importing',
                                     'imported', 'failed', 'reset')),
    -- Where a running import is: `rows` or `files`.
    stage           varchar(16),
    archive_key     text,
    archive_name    text,
    archive_size    bigint      NOT NULL DEFAULT 0,
    sealed          boolean     NOT NULL DEFAULT false,
    -- Give every imported row a new uuid: a copy of a workspace into the
    -- server that still holds it.
    regenerate_uuids boolean    NOT NULL DEFAULT false,
    -- The preview after a check, the final report after the import.
    report          jsonb,
    -- Why the last check or the import stopped: `{code, message, args}`.
    error           jsonb,
    -- Accounts the import created (new people and placeholders), which a
    -- reset removes.
    created_people  uuid[]      NOT NULL DEFAULT '{}',
    files_restored  integer     NOT NULL DEFAULT 0,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    checked_at      timestamptz,
    confirmed_by    uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    confirmed_at    timestamptz,
    -- Set in the transaction that loads the rows: the import committed.
    applied_at      timestamptz,
    finished_at     timestamptz,
    reset_by        uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    reset_at        timestamptz,
    -- The 30-day prune dropped the archive and the report's detail.
    pruned_at       timestamptz
);

-- One live import per workspace.
CREATE UNIQUE INDEX workspace_import_jobs_one_live
    ON public.workspace_import_jobs (workspace_id)
    WHERE state IN ('uploaded', 'checking', 'previewed', 'importing');
CREATE INDEX idx_workspace_import_jobs_workspace
    ON public.workspace_import_jobs (workspace_id, created_at DESC);
CREATE INDEX idx_workspace_import_jobs_requested_by
    ON public.workspace_import_jobs (requested_by) WHERE requested_by IS NOT NULL;
CREATE INDEX idx_workspace_import_jobs_confirmed_by
    ON public.workspace_import_jobs (confirmed_by) WHERE confirmed_by IS NOT NULL;
CREATE INDEX idx_workspace_import_jobs_reset_by
    ON public.workspace_import_jobs (reset_by) WHERE reset_by IS NOT NULL;

ALTER TABLE public.workspace_import_jobs OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_import_jobs TO nosdesk_app;
ALTER TABLE public.workspace_import_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_import_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_import_jobs_workspace_isolation ON public.workspace_import_jobs
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

-- A running check or import marks its job alive every 30 seconds
-- (`updated_at`) and records where it is (`stage`, `files_restored`). The
-- audit log skips an update that changes only those, as it does for
-- import_jobs. The report names people, so the log keeps only whether it
-- changed.
CREATE TRIGGER tr_audit_workspace_import_jobs AFTER INSERT OR DELETE ON public.workspace_import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'report', 'error', 'created_people');
CREATE TRIGGER tr_audit_workspace_import_jobs_upd AFTER UPDATE ON public.workspace_import_jobs
    FOR EACH ROW
    WHEN ((to_jsonb(old.*) - 'updated_at' - 'stage' - 'files_restored')
          IS DISTINCT FROM (to_jsonb(new.*) - 'updated_at' - 'stage' - 'files_restored'))
    EXECUTE FUNCTION public.audit_log_trigger('id', 'report', 'error', 'created_people');
