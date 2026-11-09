-- Restore previous values: a job made from another import's previous
-- values names that import, so each detail page can link to the other and a
-- second press finds the open draft. The key includes the workspace, so a
-- job only ever names one of its own workspace's jobs.
ALTER TABLE public.import_jobs
    ADD COLUMN restore_of uuid,
    ADD CONSTRAINT import_jobs_restore_of_fkey FOREIGN KEY (workspace_id, restore_of)
        REFERENCES public.import_jobs (workspace_id, id) ON DELETE SET NULL (restore_of);

CREATE INDEX idx_import_jobs_restore_of
    ON public.import_jobs (workspace_id, restore_of) WHERE restore_of IS NOT NULL;
