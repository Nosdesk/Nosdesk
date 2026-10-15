DROP TABLE IF EXISTS public.import_value_answers;
DROP INDEX IF EXISTS public.import_jobs_workspace_id_id_key;

DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE OR UPDATE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'id', 'summary', 'source', 'mapping', 'options', 'row_results'
    );

ALTER TABLE public.import_jobs
    DROP COLUMN record_ids,
    DROP COLUMN answers;
