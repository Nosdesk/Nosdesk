-- User contact fields, phase A.
--
-- `user_field_schema`: the workspace's custom-field definitions for users (a
-- JSON-Schema subset, validated by services::custom_fields). Override-only: a
-- row exists once an admin customises it; otherwise reads fall back to a code
-- default, so no per-workspace seed/backfill is needed here.
--
-- `user_profiles`: a per-(user × workspace) contact record holding the SCIM-
-- Enterprise standard columns (job_title, organization, department) plus the
-- custom-field values JSONB. `directory_synced` marks the standard columns as
-- Graph-owned (read-only) for that user. Multi-valued phones/addresses land in
-- phase B. Both tables are workspace-scoped.
--
-- New empty tables: the audit triggers only fire on real runtime writes (which
-- carry app.workspace_id via TenantConn), so there is no backfill to disable.

CREATE TABLE public.user_field_schema (
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    schema jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT user_field_schema_pkey PRIMARY KEY (workspace_id)
);

ALTER TABLE ONLY public.user_field_schema
    ADD CONSTRAINT user_field_schema_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_field_schema
    ADD CONSTRAINT user_field_schema_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

ALTER TABLE ONLY public.user_field_schema FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_field_schema OWNER TO nosdesk_admin;
ALTER TABLE public.user_field_schema ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_field_schema_workspace_isolation ON public.user_field_schema
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_field_schema TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_field_schema
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_field_schema AFTER INSERT OR DELETE OR UPDATE ON public.user_field_schema
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('workspace_id');


CREATE TABLE public.user_profiles (
    user_uuid uuid NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    job_title character varying(255),
    organization character varying(255),
    department character varying(255),
    custom_fields jsonb DEFAULT '{}'::jsonb NOT NULL,
    directory_synced boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    -- workspace_id leads the composite key to satisfy the tenant-table
    -- unique-index lint (the row is unique per user per workspace either way).
    CONSTRAINT user_profiles_pkey PRIMARY KEY (workspace_id, user_uuid)
);

ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_user_uuid_fkey
    FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

CREATE INDEX idx_user_profiles_user ON public.user_profiles USING btree (user_uuid);

ALTER TABLE ONLY public.user_profiles FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_profiles OWNER TO nosdesk_admin;
ALTER TABLE public.user_profiles ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_profiles_workspace_isolation ON public.user_profiles
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_profiles TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_profiles
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_profiles AFTER INSERT OR DELETE OR UPDATE ON public.user_profiles
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('user_uuid');
