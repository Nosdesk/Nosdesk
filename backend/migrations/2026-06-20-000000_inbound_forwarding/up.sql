-- Inbound forwarding addresses: the opaque-token router for forwarding-based
-- email ingestion (the hosted inbound path).
--
-- A customer forwards their support mailbox to <token>@inbound.<domain>; SES
-- receives it and the webhook resolves <token> to the owning workspace +
-- channel, then runs the existing channels parse pipeline. The token is an
-- unguessable capability rather than the workspace slug: a guessable address
-- would let anyone inject mail into a known workspace's queue. One row per
-- forwarding address; a channel can own more than one (per-inbox split) later
-- without a schema change. `status` carries 'active'/'retired' so a rotated
-- address is invalidated while staying on record.
--
-- Resolving a token is a pre-tenant, cross-workspace lookup (the webhook has
-- no workspace context until the token resolves), so the webhook reads this
-- table on a system/background connection; the token's unguessability is the
-- access control and RLS is the defence-in-depth backstop for app-path reads.
--
-- New, empty table: no backfill, so no audit-trigger backfill trap.

CREATE TABLE public.inbound_addresses (
    id integer NOT NULL,
    token character varying(64) NOT NULL,
    channel_id integer NOT NULL,
    status character varying(16) DEFAULT 'active'::character varying NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT inbound_addresses_status_check
        CHECK (status::text = ANY (ARRAY['active'::text, 'retired'::text]))
);

CREATE SEQUENCE public.inbound_addresses_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.inbound_addresses_id_seq OWNED BY public.inbound_addresses.id;
ALTER TABLE ONLY public.inbound_addresses
    ALTER COLUMN id SET DEFAULT nextval('public.inbound_addresses_id_seq'::regclass);

ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_pkey PRIMARY KEY (id);
ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_token_key UNIQUE (token);

ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_channel_id_fkey
    FOREIGN KEY (channel_id) REFERENCES public.channels(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.inbound_addresses
    ADD CONSTRAINT inbound_addresses_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

CREATE INDEX idx_inbound_addresses_channel ON public.inbound_addresses USING btree (channel_id);

ALTER TABLE ONLY public.inbound_addresses FORCE ROW LEVEL SECURITY;
ALTER TABLE public.inbound_addresses OWNER TO nosdesk_admin;
ALTER SEQUENCE public.inbound_addresses_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.inbound_addresses ENABLE ROW LEVEL SECURITY;

CREATE POLICY inbound_addresses_workspace_isolation ON public.inbound_addresses
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.inbound_addresses TO nosdesk_app;
GRANT ALL ON SEQUENCE public.inbound_addresses_id_seq TO nosdesk_app;
