-- The scrubbed values aren't restored; only the trigger goes back.
DROP TRIGGER tr_audit_webhooks ON public.webhooks;
CREATE TRIGGER tr_audit_webhooks AFTER INSERT OR DELETE OR UPDATE ON public.webhooks
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');
