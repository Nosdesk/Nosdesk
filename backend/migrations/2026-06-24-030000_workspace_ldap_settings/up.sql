-- P1 (LDAP/directory integration): one provider-agnostic LDAP config row per
-- workspace, modeled on workspace_email_settings. The bind password is stored
-- KEK-encrypted (framed AES-256-GCM blob + kek_id sidecar, workspace_id bound
-- into the AAD) and redacted from the audit log via the trigger exclude list.
-- New, empty table: no backfill, so the audit trigger only fires on live writes.
--
-- Flexible/many-valued config (attribute mappings, group model, provisioning
-- policy) lives in JSONB so the 8 provider dialects need no schema branches;
-- the connection/bind/search essentials are typed columns. The mutable sync
-- cursor is a separate table added in P3.

CREATE TABLE public.workspace_ldap_settings (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    enabled boolean DEFAULT false NOT NULL,
    -- Connection
    host character varying(255) DEFAULT ''::character varying NOT NULL,
    port integer DEFAULT 636 NOT NULL,
    tls_mode character varying(16) DEFAULT 'ldaps'::character varying NOT NULL,
    verify_certs boolean DEFAULT true NOT NULL,
    ca_cert_pem text,
    follow_referrals boolean DEFAULT false NOT NULL,
    connect_timeout_secs integer DEFAULT 5 NOT NULL,
    -- Bind / auth (service account)
    auth_mode character varying(16) DEFAULT 'simple_bind'::character varying NOT NULL,
    bind_dn character varying(512) DEFAULT ''::character varying NOT NULL,
    encrypted_bind_password bytea,
    encrypted_kek_id smallint,
    -- User search
    user_base_dn character varying(512) DEFAULT ''::character varying NOT NULL,
    username_attribute character varying(64) DEFAULT 'sAMAccountName'::character varying NOT NULL,
    user_filter text DEFAULT ''::text NOT NULL,
    page_size integer DEFAULT 500 NOT NULL,
    -- Mappings + group model + provisioning policy (provider-agnostic JSONB)
    attribute_map jsonb DEFAULT '{}'::jsonb NOT NULL,
    group_config jsonb DEFAULT '{}'::jsonb NOT NULL,
    provisioning jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_ldap_settings_pkey PRIMARY KEY (workspace_id),
    CONSTRAINT workspace_ldap_settings_tls_mode_check
        CHECK (((tls_mode)::text = ANY ((ARRAY['ldaps', 'starttls', 'plain'])::text[]))),
    CONSTRAINT workspace_ldap_settings_auth_mode_check
        CHECK (((auth_mode)::text = ANY ((ARRAY['simple_bind', 'mtls'])::text[])))
);
ALTER TABLE ONLY public.workspace_ldap_settings
    ADD CONSTRAINT workspace_ldap_settings_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.workspace_ldap_settings FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_ldap_settings OWNER TO nosdesk_admin;
ALTER TABLE public.workspace_ldap_settings ENABLE ROW LEVEL SECURITY;
CREATE POLICY workspace_ldap_settings_workspace_isolation ON public.workspace_ldap_settings
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_ldap_settings TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.workspace_ldap_settings
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_workspace_ldap_settings
    AFTER INSERT OR DELETE OR UPDATE ON public.workspace_ldap_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_bind_password');
