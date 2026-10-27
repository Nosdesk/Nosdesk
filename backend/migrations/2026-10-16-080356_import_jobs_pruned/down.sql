DROP INDEX IF EXISTS public.idx_import_jobs_workspace_created;
ALTER TABLE public.import_jobs DROP COLUMN pruned_at;
