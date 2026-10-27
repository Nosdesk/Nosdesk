DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE OR UPDATE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'summary');

ALTER TABLE public.import_jobs
    DROP COLUMN row_results,
    DROP COLUMN options,
    DROP COLUMN mapping,
    DROP COLUMN source;
