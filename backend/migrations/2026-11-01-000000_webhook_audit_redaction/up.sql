-- A webhook's signing secret and its custom header values (which commonly
-- carry an Authorization value) stay out of the audit log, as every other
-- credential's columns do: the trigger records only whether they changed.
DROP TRIGGER tr_audit_webhooks ON public.webhooks;
CREATE TRIGGER tr_audit_webhooks AFTER INSERT OR DELETE OR UPDATE ON public.webhooks
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'secret', 'headers');

-- And out of the rows already written.
UPDATE public.audit_log
   SET before_jsonb = before_jsonb - 'secret' - 'headers',
       after_jsonb = after_jsonb - 'secret' - 'headers'
 WHERE table_name = 'webhooks'
   AND (before_jsonb ?| array['secret', 'headers'] OR after_jsonb ?| array['secret', 'headers']);
