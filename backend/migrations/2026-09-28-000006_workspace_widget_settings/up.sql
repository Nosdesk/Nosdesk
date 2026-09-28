-- The embeddable help widget (docs/plans/requester-phase3.md, "Widget"). One
-- row per workspace: whether it's on, which sites may embed it (CSP
-- frame-ancestors), whether anonymous visitors get the help centre and guest
-- form, and the secret a site signs visitor identities with (KEK-sealed).
CREATE TABLE public.workspace_widget_settings (
    id                 serial      PRIMARY KEY,
    enabled            boolean     NOT NULL DEFAULT false,
    allowed_origins    text[]      NOT NULL DEFAULT '{}',
    allow_anonymous    boolean     NOT NULL DEFAULT true,
    encrypted_secret   bytea,
    encrypted_kek_id   smallint,
    workspace_id       integer     NOT NULL UNIQUE
                       DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                       REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE public.workspace_widget_settings OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_widget_settings TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.workspace_widget_settings_id_seq TO nosdesk_app;
ALTER TABLE public.workspace_widget_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_widget_settings FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_widget_settings_workspace_isolation ON public.workspace_widget_settings
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
CREATE TRIGGER tr_audit_workspace_widget_settings AFTER INSERT OR DELETE OR UPDATE ON public.workspace_widget_settings
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'encrypted_secret');
