-- Security (review must-fix): never allow a cleartext "plain" LDAP bind -- it
-- ships the service-account + end-user passwords over an unencrypted socket
-- (RFC 4513 §5.1.3). Tighten the tls_mode check to LDAPS / StartTLS only. The
-- app layer (connector + admin validator) already rejects "plain"; this closes
-- it at the DB too. New feature, so no existing row carries "plain".
ALTER TABLE public.workspace_ldap_settings
    DROP CONSTRAINT workspace_ldap_settings_tls_mode_check;
ALTER TABLE public.workspace_ldap_settings
    ADD CONSTRAINT workspace_ldap_settings_tls_mode_check
    CHECK (((tls_mode)::text = ANY ((ARRAY['ldaps', 'starttls'])::text[])));
