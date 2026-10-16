-- When the 30-day prune dropped the job's file and per-row detail. The
-- counts, options, mapping and record ids stay for the life of the
-- workspace.
ALTER TABLE public.import_jobs ADD COLUMN pruned_at timestamptz;

-- Recent imports lists a workspace's newest jobs.
CREATE INDEX idx_import_jobs_workspace_created
    ON public.import_jobs (workspace_id, created_at DESC);
