-- Asset model catalog (NetBox-style Manufacturer -> Model -> Asset).
--
-- A `manufacturer` is a make (Apple, Dell). An `asset_model` is a real
-- make+model ("MacBook Pro 16 2023") that captures the kind and a set of
-- default attributes once; an asset references one via `assets.model_id`
-- and is stamped with the manufacturer/model/kind/specs at assignment
-- time (copy-at-assignment, like NetBox: edits to a model do not rewrite
-- existing assets). Model-less assets keep working with hand-typed
-- manufacturer/model text and a directly-set kind.
--
-- All new, empty tables; the audit trigger only fires on real writes, so
-- there is no backfill to disable it for. `assets.model_id` is added
-- nullable with no backfill (DDL doesn't fire the row trigger).

-- The catalog is workspace config (like asset_kinds), not a sync
-- aggregate: pickers re-fetch via the frontend cache, and coverage lives
-- in the audit_log triggers below. No sync_aggregate enum value needed.

-- ---------------------------------------------------------------------
-- manufacturers
-- ---------------------------------------------------------------------
CREATE TABLE public.manufacturers (
    id integer NOT NULL,
    name character varying(255) NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT manufacturers_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.manufacturers_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.manufacturers_id_seq OWNED BY public.manufacturers.id;
ALTER TABLE ONLY public.manufacturers
    ALTER COLUMN id SET DEFAULT nextval('public.manufacturers_id_seq'::regclass);

ALTER TABLE ONLY public.manufacturers
    ADD CONSTRAINT manufacturers_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);
ALTER TABLE ONLY public.manufacturers
    ADD CONSTRAINT manufacturers_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

-- One name per workspace. workspace_id leads to satisfy the tenant-table
-- unique-index lint.
CREATE UNIQUE INDEX uq_manufacturers_workspace_name
    ON public.manufacturers USING btree (workspace_id, name);

ALTER TABLE ONLY public.manufacturers FORCE ROW LEVEL SECURITY;
ALTER TABLE public.manufacturers OWNER TO nosdesk_admin;
ALTER SEQUENCE public.manufacturers_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.manufacturers ENABLE ROW LEVEL SECURITY;

CREATE POLICY manufacturers_workspace_isolation ON public.manufacturers
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.manufacturers TO nosdesk_app;
GRANT ALL ON SEQUENCE public.manufacturers_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.manufacturers
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_manufacturers AFTER INSERT OR DELETE OR UPDATE ON public.manufacturers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------
-- asset_models (the "device type")
-- ---------------------------------------------------------------------
CREATE TABLE public.asset_models (
    id integer NOT NULL,
    manufacturer_id integer NOT NULL,
    name character varying(255) NOT NULL,
    -- The model's category/role. References asset_kinds.slug, validated in
    -- the app layer like assets.kind (no DB FK, slug is per-workspace).
    kind character varying(64) NOT NULL,
    part_number character varying(255),
    -- Specs stamped onto new assets of this model. Validated against the
    -- kind's user-owned attribute schema.
    default_attributes jsonb DEFAULT '{}'::jsonb NOT NULL,
    notes text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT asset_models_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.asset_models_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.asset_models_id_seq OWNED BY public.asset_models.id;
ALTER TABLE ONLY public.asset_models
    ALTER COLUMN id SET DEFAULT nextval('public.asset_models_id_seq'::regclass);

-- RESTRICT: a manufacturer with models must have them cleared first, so a
-- delete can't silently orphan a catalog.
ALTER TABLE ONLY public.asset_models
    ADD CONSTRAINT asset_models_manufacturer_id_fkey
    FOREIGN KEY (manufacturer_id) REFERENCES public.manufacturers(id) ON DELETE RESTRICT;
ALTER TABLE ONLY public.asset_models
    ADD CONSTRAINT asset_models_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);
ALTER TABLE ONLY public.asset_models
    ADD CONSTRAINT asset_models_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

CREATE UNIQUE INDEX uq_asset_models_workspace_mfr_name
    ON public.asset_models USING btree (workspace_id, manufacturer_id, name);
CREATE INDEX idx_asset_models_manufacturer ON public.asset_models USING btree (manufacturer_id);
CREATE INDEX idx_asset_models_kind ON public.asset_models USING btree (workspace_id, kind);

ALTER TABLE ONLY public.asset_models FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_models OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_models_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.asset_models ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_models_workspace_isolation ON public.asset_models
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_models TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_models_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.asset_models
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_asset_models AFTER INSERT OR DELETE OR UPDATE ON public.asset_models
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------
-- assets.model_id
-- ---------------------------------------------------------------------
ALTER TABLE public.assets ADD COLUMN model_id integer;
ALTER TABLE ONLY public.assets
    ADD CONSTRAINT assets_model_id_fkey
    FOREIGN KEY (model_id) REFERENCES public.asset_models(id) ON DELETE SET NULL;
CREATE INDEX idx_assets_model ON public.assets USING btree (model_id);
