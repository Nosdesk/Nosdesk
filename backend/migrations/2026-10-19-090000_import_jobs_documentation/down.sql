-- One audit trigger for every change again.
DROP TRIGGER IF EXISTS tr_audit_import_jobs_upd ON public.import_jobs;
DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE OR UPDATE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'id', 'summary', 'source', 'mapping', 'options', 'row_results', 'answers', 'record_ids'
    );

-- Documentation jobs and the checking status can't exist under the old
-- constraints. The pages a documentation import wrote stay. The audit
-- trigger needs a workspace context a migration doesn't have.
ALTER TABLE public.import_jobs DISABLE TRIGGER tr_audit_import_jobs;
DELETE FROM public.import_jobs WHERE job_type = 'documentation';
UPDATE public.import_jobs SET status = 'parsed' WHERE status = 'checking';
ALTER TABLE public.import_jobs ENABLE TRIGGER tr_audit_import_jobs;

ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_job_type_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets'));

ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_status_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_status_check
    CHECK (status IN ('parsed', 'dry_run_done', 'committing', 'done', 'failed'));

DROP INDEX IF EXISTS public.idx_import_jobs_status;
CREATE INDEX idx_import_jobs_status ON public.import_jobs USING btree (status)
    WHERE status IN ('parsed', 'dry_run_done', 'committing');
