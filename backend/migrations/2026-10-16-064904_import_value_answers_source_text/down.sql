DROP TRIGGER IF EXISTS tr_audit_import_value_answers ON public.import_value_answers;
CREATE TRIGGER tr_audit_import_value_answers AFTER INSERT OR DELETE OR UPDATE ON public.import_value_answers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'normalized_value', 'answer');

ALTER TABLE public.import_value_answers
    DROP COLUMN created,
    DROP COLUMN source_text;
