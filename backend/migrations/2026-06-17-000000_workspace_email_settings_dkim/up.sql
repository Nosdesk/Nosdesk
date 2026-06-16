-- Verified-domain (self-managed DKIM) sending mode for workspace_email_settings.
--
-- The hosted model: a workspace sends from its verified domain through the
-- instance relay (SES), DKIM-signed d=domain, so DMARC passes on DKIM
-- alignment alone. These columns hold the per-domain signing material and the
-- verification state. `sending_mode` discriminates how a workspace sends:
--   * fallback        - the instance/env identity (no per-workspace sending)
--   * verified_domain - DKIM-signed via the instance relay (hosted model)
--   * smtp_relay      - the workspace's own relay (the existing smtp_* columns)
--
-- Column-add on an empty table (the feature isn't live yet), so no backfill and
-- no audit-trigger trap. The DKIM private key is KEK-encrypted like the SMTP
-- password, with its own kek_id sidecar, and is redacted from the audit log
-- (the trigger is recreated below to add it to the exclude list).

ALTER TABLE public.workspace_email_settings
    ADD COLUMN sending_mode character varying(16) NOT NULL DEFAULT 'fallback',
    ADD COLUMN sending_domain character varying(255),
    ADD COLUMN dkim_selector character varying(63),
    ADD COLUMN dkim_algorithm character varying(16),
    ADD COLUMN encrypted_dkim_private_key bytea,
    ADD COLUMN dkim_kek_id smallint,
    ADD COLUMN verification_status character varying(16) NOT NULL DEFAULT 'unverified',
    ADD COLUMN verified_at timestamp with time zone;

ALTER TABLE public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_sending_mode_check
    CHECK (sending_mode::text = ANY (ARRAY['fallback'::text, 'verified_domain'::text, 'smtp_relay'::text]));

ALTER TABLE public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_verification_status_check
    CHECK (verification_status::text = ANY (ARRAY['unverified'::text, 'pending'::text, 'verified'::text, 'failed'::text]));

ALTER TABLE public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_dkim_algorithm_check
    CHECK (dkim_algorithm IS NULL OR dkim_algorithm::text = ANY (ARRAY['rsa'::text, 'ed25519'::text]));

-- Recreate the audit trigger so the DKIM private key joins the SMTP password
-- in the redaction list (logged only as <col>_changed: bool).
DROP TRIGGER tr_audit_workspace_email_settings ON public.workspace_email_settings;
CREATE TRIGGER tr_audit_workspace_email_settings
    AFTER INSERT OR UPDATE OR DELETE ON public.workspace_email_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_smtp_password', 'encrypted_dkim_private_key');
