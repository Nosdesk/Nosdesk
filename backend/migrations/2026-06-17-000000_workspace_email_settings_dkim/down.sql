-- Restore the audit trigger to redact only the SMTP password.
DROP TRIGGER tr_audit_workspace_email_settings ON public.workspace_email_settings;
CREATE TRIGGER tr_audit_workspace_email_settings
    AFTER INSERT OR UPDATE OR DELETE ON public.workspace_email_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_smtp_password');

ALTER TABLE public.workspace_email_settings
    DROP CONSTRAINT IF EXISTS workspace_email_settings_dkim_algorithm_check,
    DROP CONSTRAINT IF EXISTS workspace_email_settings_verification_status_check,
    DROP CONSTRAINT IF EXISTS workspace_email_settings_sending_mode_check,
    DROP COLUMN IF EXISTS verified_at,
    DROP COLUMN IF EXISTS verification_status,
    DROP COLUMN IF EXISTS dkim_kek_id,
    DROP COLUMN IF EXISTS encrypted_dkim_private_key,
    DROP COLUMN IF EXISTS dkim_algorithm,
    DROP COLUMN IF EXISTS dkim_selector,
    DROP COLUMN IF EXISTS sending_domain,
    DROP COLUMN IF EXISTS sending_mode;
