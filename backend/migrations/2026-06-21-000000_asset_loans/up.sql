-- Device loaning: a first-class loan ledger.
--
-- A loan is a span: an asset is in a borrower's custody from `loaned_at` until
-- `returned_at`, optionally with a `due_back` date, optionally against the
-- ticket that prompted it. The ledger is the source of truth for "who has what,
-- until when, returned or not"; `assets.status = 'on_loan'` is a denormalised
-- cache kept in step by the issue/return flow, and `asset_lifecycle_events`
-- stays the unified status timeline (the issue/return transitions reference the
-- loan via metadata.loan_id). At most one active (unreturned) loan per asset.
--
-- New, empty table: the audit trigger only ever fires on real loan writes, so
-- there is no backfill to disable it for.

-- Register the loan aggregate on the sync_actions enum so loan events can be
-- recorded on the pool. Safe inside the migration transaction (PG12+): the
-- value is only added here, never used until a later runtime emit. (The down
-- migration can't remove it; Postgres doesn't drop enum values, which is the
-- standard, harmless limitation.)
ALTER TYPE public.sync_aggregate ADD VALUE IF NOT EXISTS 'asset_loan';

CREATE TABLE public.asset_loans (
    id integer NOT NULL,
    asset_id integer NOT NULL,
    borrower_user_uuid uuid NOT NULL,
    loaned_at timestamp with time zone DEFAULT now() NOT NULL,
    due_back date,
    returned_at timestamp with time zone,
    ticket_id integer,
    status_before character varying(32) NOT NULL,
    notes text,
    actor_uuid uuid,
    returned_by_uuid uuid,
    due_soon_notified_at timestamp with time zone,
    overdue_notified_at timestamp with time zone,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT asset_loans_pkey PRIMARY KEY (id)
);

CREATE SEQUENCE public.asset_loans_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.asset_loans_id_seq OWNED BY public.asset_loans.id;
ALTER TABLE ONLY public.asset_loans
    ALTER COLUMN id SET DEFAULT nextval('public.asset_loans_id_seq'::regclass);

ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_asset_id_fkey
    FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_borrower_user_uuid_fkey
    FOREIGN KEY (borrower_user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_ticket_id_fkey
    FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_actor_uuid_fkey
    FOREIGN KEY (actor_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_returned_by_uuid_fkey
    FOREIGN KEY (returned_by_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;
ALTER TABLE ONLY public.asset_loans
    ADD CONSTRAINT asset_loans_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

-- At most one active loan per asset (a device can't be in two hands at once).
-- workspace_id leads the key to satisfy the tenant-table unique-index lint; it
-- is redundant since asset_id is globally unique, but keeps the invariant clean
-- without an allowlist entry.
CREATE UNIQUE INDEX uq_asset_loans_active_per_asset
    ON public.asset_loans USING btree (workspace_id, asset_id) WHERE (returned_at IS NULL);

CREATE INDEX idx_asset_loans_asset ON public.asset_loans USING btree (asset_id);
CREATE INDEX idx_asset_loans_borrower ON public.asset_loans USING btree (borrower_user_uuid);
CREATE INDEX idx_asset_loans_ticket ON public.asset_loans USING btree (ticket_id);
-- Reminder scan: open loans that carry a due date.
CREATE INDEX idx_asset_loans_due_open ON public.asset_loans USING btree (due_back)
    WHERE (returned_at IS NULL AND due_back IS NOT NULL);

ALTER TABLE ONLY public.asset_loans FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_loans OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_loans_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.asset_loans ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_loans_workspace_isolation ON public.asset_loans
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_loans TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_loans_id_seq TO nosdesk_app;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.asset_loans
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_asset_loans AFTER INSERT OR DELETE OR UPDATE ON public.asset_loans
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');
