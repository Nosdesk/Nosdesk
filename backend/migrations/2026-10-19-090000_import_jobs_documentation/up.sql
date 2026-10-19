-- Documentation imports run as import jobs: a zip of Markdown is checked
-- in the background (`checking`) and committed in the background
-- (`committing`, which already exists).
ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_job_type_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets', 'documentation'));

ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_status_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_status_check
    CHECK (status IN ('parsed', 'checking', 'dry_run_done', 'committing', 'done', 'failed'));

DROP INDEX IF EXISTS public.idx_import_jobs_status;
CREATE INDEX idx_import_jobs_status ON public.import_jobs USING btree (status)
    WHERE status IN ('parsed', 'checking', 'dry_run_done', 'committing');
