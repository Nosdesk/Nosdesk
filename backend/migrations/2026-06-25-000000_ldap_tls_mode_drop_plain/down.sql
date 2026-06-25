ALTER TABLE public.workspace_ldap_settings
    DROP CONSTRAINT workspace_ldap_settings_tls_mode_check;
ALTER TABLE public.workspace_ldap_settings
    ADD CONSTRAINT workspace_ldap_settings_tls_mode_check
    CHECK (((tls_mode)::text = ANY ((ARRAY['ldaps', 'starttls', 'plain'])::text[])));
