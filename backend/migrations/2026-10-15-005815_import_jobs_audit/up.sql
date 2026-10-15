DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
-- An import job records who bulk-wrote users, assets or tickets. Audit it
-- like other admin config; the summary carries row errors (other people's
-- emails among them), so only whether it changed is kept.
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE OR UPDATE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'summary');
