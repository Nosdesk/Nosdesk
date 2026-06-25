-- User contact fields, phase B: multi-valued typed phones + addresses.
--
-- vCard TEL / ADR realised as satellite tables like user_emails, but
-- workspace-scoped (the extended contact card is per workspace; email stays the
-- global identity anchor). `source` mirrors user_emails: NULL = manual, a
-- provider name (e.g. 'microsoft') = sync-owned/read-only. One primary per
-- (workspace, user) per table, enforced by a partial unique index.

CREATE TABLE public.user_phone_numbers (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    phone character varying(64) NOT NULL,
    phone_type character varying(16) DEFAULT 'work'::character varying NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    source character varying(32),
    label character varying(100),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT user_phone_numbers_pkey PRIMARY KEY (id),
    CONSTRAINT user_phone_numbers_type_check CHECK (((phone_type)::text = ANY ((ARRAY['work', 'mobile', 'other'])::text[])))
);
CREATE SEQUENCE public.user_phone_numbers_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.user_phone_numbers_id_seq OWNED BY public.user_phone_numbers.id;
ALTER TABLE ONLY public.user_phone_numbers
    ALTER COLUMN id SET DEFAULT nextval('public.user_phone_numbers_id_seq'::regclass);
ALTER TABLE ONLY public.user_phone_numbers
    ADD CONSTRAINT user_phone_numbers_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_phone_numbers
    ADD CONSTRAINT user_phone_numbers_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_phone_numbers
    ADD CONSTRAINT user_phone_numbers_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
CREATE INDEX idx_user_phone_numbers_user ON public.user_phone_numbers USING btree (workspace_id, user_uuid);
CREATE UNIQUE INDEX uq_user_phone_numbers_primary ON public.user_phone_numbers USING btree (workspace_id, user_uuid) WHERE is_primary;
ALTER TABLE ONLY public.user_phone_numbers FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_phone_numbers OWNER TO nosdesk_admin;
ALTER SEQUENCE public.user_phone_numbers_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.user_phone_numbers ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_phone_numbers_workspace_isolation ON public.user_phone_numbers
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_phone_numbers TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_phone_numbers_id_seq TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_phone_numbers
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_phone_numbers AFTER INSERT OR DELETE OR UPDATE ON public.user_phone_numbers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');


CREATE TABLE public.user_addresses (
    id integer NOT NULL,
    user_uuid uuid NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    address_type character varying(16) DEFAULT 'work'::character varying NOT NULL,
    is_primary boolean DEFAULT false NOT NULL,
    street character varying(255),
    city character varying(128),
    region character varying(128),
    postal_code character varying(32),
    country character varying(128),
    source character varying(32),
    label character varying(100),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    created_by uuid,
    CONSTRAINT user_addresses_pkey PRIMARY KEY (id),
    CONSTRAINT user_addresses_type_check CHECK (((address_type)::text = ANY ((ARRAY['work', 'home', 'other'])::text[])))
);
CREATE SEQUENCE public.user_addresses_id_seq
    AS integer START WITH 1 INCREMENT BY 1 NO MINVALUE NO MAXVALUE CACHE 1;
ALTER SEQUENCE public.user_addresses_id_seq OWNED BY public.user_addresses.id;
ALTER TABLE ONLY public.user_addresses
    ALTER COLUMN id SET DEFAULT nextval('public.user_addresses_id_seq'::regclass);
ALTER TABLE ONLY public.user_addresses
    ADD CONSTRAINT user_addresses_user_uuid_fkey FOREIGN KEY (user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_addresses
    ADD CONSTRAINT user_addresses_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.user_addresses
    ADD CONSTRAINT user_addresses_created_by_fkey FOREIGN KEY (created_by) REFERENCES public.users(uuid) ON DELETE SET NULL;
CREATE INDEX idx_user_addresses_user ON public.user_addresses USING btree (workspace_id, user_uuid);
CREATE UNIQUE INDEX uq_user_addresses_primary ON public.user_addresses USING btree (workspace_id, user_uuid) WHERE is_primary;
ALTER TABLE ONLY public.user_addresses FORCE ROW LEVEL SECURITY;
ALTER TABLE public.user_addresses OWNER TO nosdesk_admin;
ALTER SEQUENCE public.user_addresses_id_seq OWNER TO nosdesk_admin;
ALTER TABLE public.user_addresses ENABLE ROW LEVEL SECURITY;
CREATE POLICY user_addresses_workspace_isolation ON public.user_addresses
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.user_addresses TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_addresses_id_seq TO nosdesk_app;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.user_addresses
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
CREATE TRIGGER tr_audit_user_addresses AFTER INSERT OR DELETE OR UPDATE ON public.user_addresses
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');
