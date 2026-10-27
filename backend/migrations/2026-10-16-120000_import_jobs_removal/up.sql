-- Remove imported assets: when it ran, who ran it, and what it did (each
-- asset deleted or kept with its reasons, and the catalog entries removed
-- or archived). It runs once per job. `remove_result` stays out of the
-- audit trigger's redaction list, so the job's audit entry carries the
-- kept list.
ALTER TABLE public.import_jobs
    ADD COLUMN removed_at timestamptz,
    ADD COLUMN removed_by uuid REFERENCES public.users(uuid) ON DELETE SET NULL,
    ADD COLUMN remove_result jsonb;

CREATE INDEX idx_import_jobs_removed_by
    ON public.import_jobs (removed_by) WHERE removed_by IS NOT NULL;
