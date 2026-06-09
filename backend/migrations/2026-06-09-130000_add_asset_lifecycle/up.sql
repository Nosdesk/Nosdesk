-- Asset lifecycle: a current `status` column on the asset plus an
-- append-only transition log. State-specific data (repair vendor /
-- RMA / offsite flag, loan recipient / due-back) lives in each
-- event's `metadata` JSONB, so new workflows need no schema change.
-- This migration also workspace-scopes serial-number uniqueness,
-- which was previously global across the whole database.

-- 1. Current status on the asset. Validated in the app layer
--    (models::AssetStatus) instead of a CHECK constraint, so adding
--    a status is a code change rather than a migration. Default
--    keeps every existing asset in service.
ALTER TABLE public.assets
    ADD COLUMN status character varying(32) DEFAULT 'in_service'::character varying NOT NULL;

CREATE INDEX idx_assets_workspace_status ON public.assets USING btree (workspace_id, status);

-- 2. Workspace-scope serial uniqueness (was a global unique index).
DROP INDEX IF EXISTS public.idx_asset_serial_unique;
CREATE UNIQUE INDEX idx_asset_serial_unique
    ON public.assets USING btree (workspace_id, serial_number)
    WHERE (serial_number IS NOT NULL);

-- 3. New sync aggregate for transition events. Not referenced in
--    this migration, so adding the enum value inside the migration
--    transaction is safe on PG 12+.
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'asset_lifecycle_event' AFTER 'asset_media';

-- 4. Append-only transition log. `ticket_id` links a transition to
--    the ticket that captured the problem (e.g. a repair); `metadata`
--    carries state-specific fields without dedicated columns.
CREATE TABLE public.asset_lifecycle_events (
    id integer NOT NULL,
    asset_id integer NOT NULL,
    from_status character varying(32),
    to_status character varying(32) NOT NULL,
    reason text,
    ticket_id integer,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    actor_uuid uuid,
    occurred_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL
);

ALTER TABLE ONLY public.asset_lifecycle_events FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_lifecycle_events OWNER TO nosdesk_admin;

CREATE SEQUENCE public.asset_lifecycle_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.asset_lifecycle_events_id_seq OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_lifecycle_events_id_seq OWNED BY public.asset_lifecycle_events.id;
ALTER TABLE ONLY public.asset_lifecycle_events ALTER COLUMN id SET DEFAULT nextval('public.asset_lifecycle_events_id_seq'::regclass);

ALTER TABLE ONLY public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_pkey PRIMARY KEY (id);

CREATE INDEX idx_asset_lifecycle_events_asset ON public.asset_lifecycle_events USING btree (asset_id, occurred_at DESC);
CREATE INDEX idx_asset_lifecycle_events_ticket ON public.asset_lifecycle_events USING btree (ticket_id) WHERE (ticket_id IS NOT NULL);

CREATE TRIGGER tr_audit_asset_lifecycle_events AFTER INSERT OR DELETE OR UPDATE ON public.asset_lifecycle_events FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

ALTER TABLE ONLY public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_actor_uuid_fkey FOREIGN KEY (actor_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;

ALTER TABLE ONLY public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

ALTER TABLE public.asset_lifecycle_events ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_lifecycle_events_workspace_isolation ON public.asset_lifecycle_events USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.asset_lifecycle_events TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_lifecycle_events_id_seq TO nosdesk_app;
