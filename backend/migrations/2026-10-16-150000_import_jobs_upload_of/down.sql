DROP INDEX IF EXISTS public.idx_import_jobs_upload_of;
ALTER TABLE public.import_jobs
    DROP CONSTRAINT IF EXISTS import_jobs_upload_of_fkey,
    DROP COLUMN upload_of;
