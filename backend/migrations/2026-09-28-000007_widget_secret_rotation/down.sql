DROP TRIGGER tr_audit_workspace_widget_settings ON public.workspace_widget_settings;
CREATE TRIGGER tr_audit_workspace_widget_settings AFTER INSERT OR DELETE OR UPDATE ON public.workspace_widget_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'encrypted_secret');
ALTER TABLE public.workspace_widget_settings
    DROP COLUMN previous_valid_until,
    DROP COLUMN previous_kek_id,
    DROP COLUMN encrypted_previous_secret,
    ADD COLUMN allow_anonymous boolean NOT NULL DEFAULT true;
