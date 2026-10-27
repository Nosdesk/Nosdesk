DROP INDEX IF EXISTS public.idx_import_jobs_removed_by;
ALTER TABLE public.import_jobs
    DROP COLUMN remove_result,
    DROP COLUMN removed_by,
    DROP COLUMN removed_at;
