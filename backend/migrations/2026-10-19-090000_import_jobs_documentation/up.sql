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

-- A background check or commit marks its job alive every 30 seconds
-- (`updated_at`, and the pages written so far in `records_committed`).
-- Those marks aren't changes anyone reviews, so the audit log skips an
-- update that changes nothing else, as plugin_trusted_publishers skips one
-- that changes nothing. Inserts, deletes and every other update are audited
-- as before.
DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'id', 'summary', 'source', 'mapping', 'options', 'row_results', 'answers', 'record_ids'
    );
CREATE TRIGGER tr_audit_import_jobs_upd AFTER UPDATE ON public.import_jobs
    FOR EACH ROW
    WHEN ((to_jsonb(old.*) - 'updated_at' - 'records_committed')
          IS DISTINCT FROM (to_jsonb(new.*) - 'updated_at' - 'records_committed'))
    EXECUTE FUNCTION public.audit_log_trigger(
        'id', 'summary', 'source', 'mapping', 'options', 'row_results', 'answers', 'record_ids'
    );
