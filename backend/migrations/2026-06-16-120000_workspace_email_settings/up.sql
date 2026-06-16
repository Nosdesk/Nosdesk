-- Per-workspace outbound email identity.
--
-- Outbound SMTP was instance-global (one EmailService::from_env shared by
-- every workspace), while inbound IMAP is already per-channel. Hosted
-- tenants could not send from their own address or relay. This table holds
-- a per-workspace sending identity (From + SMTP transport); the global env
-- config stays the fallback, so single-tenant self-host is unchanged.
--
-- New, empty table: no backfill, so the audit trigger only ever fires on
-- live workspace-pinned writes and there is no NDX01 actor-context trap.
-- The SMTP password is stored KEK-encrypted (the same framed AES-256-GCM
-- blob + kek_id sidecar as channel_credentials.encrypted_value) and is
-- redacted from the audit log via the trigger's exclude list.

CREATE TABLE public.workspace_email_settings (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    enabled boolean DEFAULT false NOT NULL,
    from_name character varying(255) DEFAULT ''::character varying NOT NULL,
    from_email character varying(320) DEFAULT ''::character varying NOT NULL,
    smtp_host character varying(255) DEFAULT ''::character varying NOT NULL,
    smtp_port integer DEFAULT 587 NOT NULL,
    smtp_security character varying(16) DEFAULT 'starttls'::character varying NOT NULL,
    smtp_username character varying(255) DEFAULT ''::character varying NOT NULL,
    encrypted_smtp_password bytea,
    encrypted_kek_id smallint,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_email_settings_pkey PRIMARY KEY (workspace_id),
    CONSTRAINT workspace_email_settings_smtp_security_check
        CHECK (smtp_security::text = ANY (ARRAY['tls'::text, 'starttls'::text, 'plaintext'::text])),
    CONSTRAINT workspace_email_settings_smtp_port_check
        CHECK (smtp_port > 0 AND smtp_port <= 65535)
);

ALTER TABLE ONLY public.workspace_email_settings FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_email_settings OWNER TO nosdesk_admin;

ALTER TABLE ONLY public.workspace_email_settings
    ADD CONSTRAINT workspace_email_settings_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

ALTER TABLE public.workspace_email_settings ENABLE ROW LEVEL SECURITY;

CREATE POLICY workspace_email_settings_workspace_isolation ON public.workspace_email_settings
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_email_settings TO nosdesk_app;

-- pk = workspace_id; encrypted_smtp_password is redacted (logged only as
-- encrypted_smtp_password_changed: bool).
CREATE TRIGGER tr_audit_workspace_email_settings
    AFTER INSERT OR DELETE OR UPDATE ON public.workspace_email_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id', 'encrypted_smtp_password');
