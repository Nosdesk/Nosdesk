-- DirSync cursor state, one row per workspace. The opaque DirSync cookie is
-- client-held (unlike Graph's server-held deltaLink), so it MUST survive
-- restarts. Operational state, not audited; RLS-isolated like the config row.
CREATE TABLE public.workspace_ldap_sync_state (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    -- Which incremental mechanism the cookie belongs to (v1: dirsync).
    mechanism character varying(16) DEFAULT 'dirsync'::character varying NOT NULL,
    -- The opaque cursor (DirSync cookie). NULL = no cursor yet -> next run is a
    -- full sync (an empty-cookie DirSync returns everything + a fresh cookie).
    cookie bytea,
    -- When the last full reconcile completed (for the nightly safety pass).
    last_full_reconcile_at timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT workspace_ldap_sync_state_pkey PRIMARY KEY (workspace_id)
);
ALTER TABLE ONLY public.workspace_ldap_sync_state
    ADD CONSTRAINT workspace_ldap_sync_state_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.workspace_ldap_sync_state FORCE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_ldap_sync_state OWNER TO nosdesk_admin;
ALTER TABLE public.workspace_ldap_sync_state ENABLE ROW LEVEL SECURITY;
CREATE POLICY workspace_ldap_sync_state_workspace_isolation ON public.workspace_ldap_sync_state
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_ldap_sync_state TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.workspace_ldap_sync_state
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
