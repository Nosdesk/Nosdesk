-- Import the next sheet: a job made from another job's workbook names the
-- job the file was first uploaded to, so the sheets already imported from
-- that upload can be told apart. The first job of an upload leaves it null.
-- The key includes the workspace, so a job only ever names one of its own
-- workspace's jobs.
ALTER TABLE public.import_jobs
    ADD COLUMN upload_of uuid,
    ADD CONSTRAINT import_jobs_upload_of_fkey FOREIGN KEY (workspace_id, upload_of)
        REFERENCES public.import_jobs (workspace_id, id) ON DELETE SET NULL (upload_of);

CREATE INDEX idx_import_jobs_upload_of
    ON public.import_jobs (workspace_id, upload_of) WHERE upload_of IS NOT NULL;
