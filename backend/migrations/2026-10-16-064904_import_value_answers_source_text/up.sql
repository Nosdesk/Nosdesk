-- A saved match keeps the value as the file first spelled it, for the Saved
-- matches page, and whether its answer links to an entry the import that
-- saved it created.
ALTER TABLE public.import_value_answers
    ADD COLUMN source_text text,
    ADD COLUMN created boolean NOT NULL DEFAULT false;

-- The spelling can be a person's email, so the audit log keeps only whether
-- it changed, like the normalized value.
DROP TRIGGER IF EXISTS tr_audit_import_value_answers ON public.import_value_answers;
CREATE TRIGGER tr_audit_import_value_answers AFTER INSERT OR DELETE OR UPDATE ON public.import_value_answers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'normalized_value', 'source_text', 'answer');
