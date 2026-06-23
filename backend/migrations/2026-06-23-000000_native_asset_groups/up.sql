-- Native asset groups: workspace-local, user-managed classification of assets
-- (e.g. "Loaner pool", "Exec laptops", "Warehouse scanners"). Tag-style UX
-- (multi-assign, assigned from the asset, surfaced as a list filter) over an
-- entity-shaped schema, so future depth (smart membership, a detail page)
-- stays additive rather than a concept migration.
--
-- Distinct from the directory-group memberships Intune/Entra sync owns: that
-- junction is renamed here from `asset_groups` to `asset_directory_memberships`
-- so this native entity can own the `asset_groups` name and the schema reads
-- unambiguously. The rename is a pure relation rename, data and the sync path
-- are untouched. (Constraint names already carry a legacy `device_groups_*`
-- prefix from an earlier rename; following that precedent we rename the table,
-- its policy and its indexes, and leave the constraint names alone.)

-- 0. Free the `asset_groups` name. The existing junction links assets to
--    directory `groups`, so name it for what it is.
ALTER TABLE public.asset_groups RENAME TO asset_directory_memberships;
ALTER POLICY asset_groups_workspace_isolation
    ON public.asset_directory_memberships RENAME TO asset_directory_memberships_workspace_isolation;
ALTER INDEX idx_asset_groups_asset RENAME TO idx_asset_directory_memberships_asset;
ALTER INDEX idx_asset_groups_group RENAME TO idx_asset_directory_memberships_group;
ALTER INDEX idx_asset_groups_external RENAME TO idx_asset_directory_memberships_external;

-- 1. Native classification entity. Mirrors ticket_categories (uuid, color,
--    display_order, audited) plus the tags soft-archive (`archived_at`).
CREATE TABLE public.asset_groups (
    id integer NOT NULL,
    uuid uuid DEFAULT gen_random_uuid() NOT NULL,
    name character varying(255) NOT NULL,
    description text,
    color character varying(7),
    display_order integer DEFAULT 0 NOT NULL,
    archived_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_groups_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.asset_groups_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.asset_groups_id_seq OWNED BY public.asset_groups.id;
ALTER TABLE ONLY public.asset_groups
    ALTER COLUMN id SET DEFAULT nextval('public.asset_groups_id_seq'::regclass);

ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT asset_groups_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_groups
    ADD CONSTRAINT asset_groups_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

-- One active group per name per workspace. workspace_id leads the key to
-- satisfy the tenant-table unique-index lint; archived rows are excluded so a
-- name frees up once archived.
CREATE UNIQUE INDEX uq_asset_groups_name_active
    ON public.asset_groups USING btree (workspace_id, lower((name)::text))
    WHERE (archived_at IS NULL);
CREATE INDEX idx_asset_groups_workspace ON public.asset_groups USING btree (workspace_id);

ALTER TABLE ONLY public.asset_groups FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_groups OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_groups_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.asset_groups ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_groups_workspace_isolation ON public.asset_groups
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_groups TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_groups_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.asset_groups
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_asset_groups AFTER INSERT OR DELETE OR UPDATE ON public.asset_groups
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- 2. Membership junction (tag-style, assigned from the asset side). High-churn,
--    so it follows ticket_tags: no audit trigger, no updated_at.
CREATE TABLE public.asset_group_assignments (
    group_id integer NOT NULL,
    asset_id integer NOT NULL,
    added_at timestamp with time zone DEFAULT now() NOT NULL,
    added_by uuid,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_group_assignments_pkey PRIMARY KEY (group_id, asset_id)
);

ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_group_id_fkey
    FOREIGN KEY (group_id) REFERENCES public.asset_groups(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_asset_id_fkey
    FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_added_by_fkey
    FOREIGN KEY (added_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_group_assignments
    ADD CONSTRAINT asset_group_assignments_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

-- pkey covers (group_id, asset_id) for group->assets; add the reverse for the
-- per-asset enrichment lookup.
CREATE INDEX idx_asset_group_assignments_asset ON public.asset_group_assignments USING btree (asset_id);

ALTER TABLE ONLY public.asset_group_assignments FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_group_assignments OWNER TO nosdesk_admin;
ALTER TABLE public.asset_group_assignments ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_group_assignments_workspace_isolation ON public.asset_group_assignments
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_group_assignments TO nosdesk_app;
