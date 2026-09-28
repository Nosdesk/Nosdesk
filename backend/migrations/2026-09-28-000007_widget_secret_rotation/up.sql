-- Widget signed identity (docs/plans/requester-phase3.md, widget hardening).
-- Rotating the signing secret keeps the previous one for a day, so a site can
-- switch over without signed-in visitors being turned away. Whether visitors
-- who aren't signed in get help is the guest access settings' call, so the
-- widget's own toggle goes.
ALTER TABLE public.workspace_widget_settings
    DROP COLUMN allow_anonymous,
    ADD COLUMN encrypted_previous_secret bytea,
    ADD COLUMN previous_kek_id smallint,
    ADD COLUMN previous_valid_until timestamptz;

DROP TRIGGER tr_audit_workspace_widget_settings ON public.workspace_widget_settings;
CREATE TRIGGER tr_audit_workspace_widget_settings AFTER INSERT OR DELETE OR UPDATE ON public.workspace_widget_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'encrypted_secret', 'encrypted_previous_secret');
