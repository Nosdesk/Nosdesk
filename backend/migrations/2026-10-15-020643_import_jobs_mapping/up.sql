-- Column mapping for imports. `source` describes the uploaded file (headers,
-- samples, suggested mapping), `mapping` maps field ids to column indexes,
-- `options` holds the match key and parsing choices, and `row_results` keeps
-- every row's outcome (written from the preview slice on).
ALTER TABLE public.import_jobs
    ADD COLUMN source JSONB,
    ADD COLUMN mapping JSONB,
    ADD COLUMN options JSONB,
    ADD COLUMN row_results JSONB;

-- The new columns carry cell values (other people's emails among them), so
-- the audit log keeps only whether they changed, as for `summary`.
DROP TRIGGER IF EXISTS tr_audit_import_jobs ON public.import_jobs;
CREATE TRIGGER tr_audit_import_jobs AFTER INSERT OR DELETE OR UPDATE ON public.import_jobs
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'id', 'summary', 'source', 'mapping', 'options', 'row_results'
    );
