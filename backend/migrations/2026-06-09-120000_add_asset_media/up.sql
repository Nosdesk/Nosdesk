-- Asset media: photos and future file-backed media attached directly
-- to an asset rather than to a ticket comment.
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'asset_media' AFTER 'asset';

CREATE TABLE public.asset_media (
    id integer NOT NULL,
    asset_id integer NOT NULL,
    url character varying(2048) NOT NULL,
    name character varying(255) NOT NULL,
    file_size bigint,
    mime_type character varying(100),
    checksum character varying(64),
    kind character varying(32) DEFAULT 'photo'::character varying NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    caption text,
    uploaded_by uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT asset_media_kind_check CHECK (((kind)::text = ANY ((ARRAY['photo'::character varying, 'document'::character varying, 'other'::character varying])::text[])))
);

ALTER TABLE ONLY public.asset_media FORCE ROW LEVEL SECURITY;
ALTER TABLE public.asset_media OWNER TO nosdesk_admin;

CREATE SEQUENCE public.asset_media_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.asset_media_id_seq OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_media_id_seq OWNED BY public.asset_media.id;
ALTER TABLE ONLY public.asset_media ALTER COLUMN id SET DEFAULT nextval('public.asset_media_id_seq'::regclass);

ALTER TABLE ONLY public.asset_media
    ADD CONSTRAINT asset_media_pkey PRIMARY KEY (id);

CREATE INDEX idx_asset_media_asset ON public.asset_media USING btree (asset_id, sort_order, created_at DESC);
CREATE INDEX idx_asset_media_uploaded_by ON public.asset_media USING btree (uploaded_by);

CREATE TRIGGER tr_audit_asset_media AFTER INSERT OR DELETE OR UPDATE ON public.asset_media FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

ALTER TABLE ONLY public.asset_media
    ADD CONSTRAINT asset_media_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.asset_media
    ADD CONSTRAINT asset_media_uploaded_by_fkey FOREIGN KEY (uploaded_by) REFERENCES public.users(uuid) ON DELETE SET NULL;

ALTER TABLE ONLY public.asset_media
    ADD CONSTRAINT asset_media_workspace_id_fkey FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

ALTER TABLE public.asset_media ENABLE ROW LEVEL SECURITY;

CREATE POLICY asset_media_workspace_isolation ON public.asset_media USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer)) WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT,INSERT,DELETE,UPDATE ON TABLE public.asset_media TO nosdesk_app;
GRANT ALL ON SEQUENCE public.asset_media_id_seq TO nosdesk_app;
