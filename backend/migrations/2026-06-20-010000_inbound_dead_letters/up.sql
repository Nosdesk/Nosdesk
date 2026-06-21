-- Unrouted inbound mail: the dead-letter log for the hosted inbound path.
--
-- Mail forwarded to an unknown <token>@inbound.<domain> can't be attributed to
-- any workspace (a mistyped forward target, a forward set up before the
-- channel was saved, or a rotated-out token), so this table is platform-level,
-- NOT workspace-scoped: there is deliberately no workspace_id and no RLS. It is
-- a diagnostic, not a quarantine, so a misconfigured forward is visible to the
-- operator instead of vanishing silently. Spam/virus-failing unknown mail is
-- dropped without a row; only scans-passing unknown mail lands here. The S3
-- lifecycle expires the referenced object on its own, so each row points at a
-- body that self-deletes.

CREATE TABLE public.inbound_dead_letters (
    id bigint NOT NULL,
    envelope_recipient character varying(320) NOT NULL,
    from_address character varying(320),
    subject text,
    s3_key text NOT NULL,
    reason character varying(32) NOT NULL,
    received_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE public.inbound_dead_letters_id_seq
    AS bigint START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.inbound_dead_letters_id_seq OWNED BY public.inbound_dead_letters.id;
ALTER TABLE ONLY public.inbound_dead_letters
    ALTER COLUMN id SET DEFAULT nextval('public.inbound_dead_letters_id_seq'::regclass);

ALTER TABLE ONLY public.inbound_dead_letters
    ADD CONSTRAINT inbound_dead_letters_pkey PRIMARY KEY (id);

CREATE INDEX idx_inbound_dead_letters_received_at
    ON public.inbound_dead_letters USING btree (received_at DESC);

ALTER TABLE public.inbound_dead_letters OWNER TO nosdesk_admin;
ALTER SEQUENCE public.inbound_dead_letters_id_seq OWNER TO nosdesk_admin;

GRANT SELECT, INSERT, DELETE ON TABLE public.inbound_dead_letters TO nosdesk_app;
GRANT ALL ON SEQUENCE public.inbound_dead_letters_id_seq TO nosdesk_app;
