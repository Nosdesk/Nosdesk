-- Asset records (#736): work done on an asset, beside the lifecycle, loan and
-- disposal ledgers. A record is a repair, an inspection, preventative
-- maintenance, a firmware update or any type a workspace adds.
--
-- A record never writes `assets.status`: moving into or out of repair stays a
-- transition. Each record carries when the work happened (`occurred_on`, and
-- `occurred_at` when the time is known), freely backdated, apart from when and
-- by whom it was entered (`recorded_at`, `recorded_by`), which never change.
--
-- No rows are seeded: the four built-in types are a catalogue in code that a
-- workspace adds from, so both tables start empty and their audit triggers
-- only ever fire on real writes.

-- ---------------------------------------------------------------------------
-- Record types
-- ---------------------------------------------------------------------------

CREATE TABLE public.asset_record_types (
    id                serial      PRIMARY KEY,
    workspace_id      integer     NOT NULL
                      DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                      REFERENCES public.workspaces(id) ON DELETE CASCADE,
    -- Stable key for the API, import templates and the catalogue; never
    -- changes once the type exists.
    slug              varchar(64) NOT NULL
                      CONSTRAINT asset_record_types_slug_check CHECK (slug ~ '^[a-z][a-z0-9_]*$'),
    -- Null on a built-in nobody renamed: clients show its catalogue name.
    label             varchar(255),
    description       text,
    icon              varchar(64),
    color             varchar(7)
                      CONSTRAINT asset_record_types_color_check CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    -- The type's own fields, in the custom-fields schema subset.
    field_schema      jsonb       NOT NULL DEFAULT '{"type": "object", "properties": {}}'::jsonb,
    -- Display and template order of field_schema's properties (jsonb sorts
    -- object keys).
    field_order       text[]      NOT NULL DEFAULT '{}',
    -- Per shared field: "hidden", "optional" or "required". Absent means
    -- optional.
    core_fields       jsonb       NOT NULL DEFAULT '{}'::jsonb,
    -- Ordered [{key, category, label?}], category one of done, not_done,
    -- no_fault, replaced.
    outcomes          jsonb       NOT NULL DEFAULT '[]'::jsonb,
    -- Asset kind slugs offered this type; null means every kind.
    applies_to_kinds  text[],
    -- The asset status whose transition opens a record of this type.
    opens_with_status varchar(32),
    is_builtin        boolean     NOT NULL DEFAULT false,
    sort_order        integer     NOT NULL DEFAULT 0,
    archived_at       timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    created_by        uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    CONSTRAINT asset_record_types_label_check CHECK (is_builtin OR label IS NOT NULL)
);
ALTER TABLE public.asset_record_types OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_record_types_id_seq OWNER TO nosdesk_admin;

CREATE UNIQUE INDEX asset_record_types_workspace_id_id_key
    ON public.asset_record_types (workspace_id, id);
CREATE UNIQUE INDEX asset_record_types_workspace_slug_key
    ON public.asset_record_types (workspace_id, slug);
-- One live type per status a transition opens a record with.
CREATE UNIQUE INDEX asset_record_types_opens_with_status_key
    ON public.asset_record_types (workspace_id, opens_with_status)
    WHERE opens_with_status IS NOT NULL AND archived_at IS NULL;

ALTER TABLE public.asset_record_types ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.asset_record_types FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_record_types_workspace_isolation ON public.asset_record_types
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_record_types TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.asset_record_types_id_seq TO nosdesk_app;

CREATE TRIGGER tr_audit_asset_record_types AFTER INSERT OR DELETE OR UPDATE ON public.asset_record_types
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------------
-- Records
-- ---------------------------------------------------------------------------

CREATE TABLE public.asset_records (
    id                   serial       PRIMARY KEY,
    workspace_id         integer      NOT NULL
                         DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                         REFERENCES public.workspaces(id) ON DELETE CASCADE,
    asset_id             integer      NOT NULL,
    record_type_id       integer      NOT NULL,
    summary              varchar(255),
    notes                text,
    -- The day the work was done, in the workspace's timezone; with the
    -- moment when it is known.
    occurred_on          date         NOT NULL
                         CONSTRAINT asset_records_occurred_on_check CHECK (occurred_on >= DATE '1900-01-01'),
    occurred_at          timestamptz,
    -- Null while the work is open.
    completed_on         date,
    completed_at         timestamptz,
    -- A key of the type's outcomes; set only once the work is complete.
    outcome              varchar(64),
    -- A vendor or external technician, or an internal one.
    vendor               varchar(255),
    performed_by_uuid    uuid         REFERENCES public.users(uuid) ON DELETE SET NULL,
    -- RMA, work order or invoice number.
    reference            varchar(255),
    cost_minor           bigint
                         CONSTRAINT asset_records_cost_minor_check CHECK (cost_minor >= 0),
    cost_currency        char(3)
                         CONSTRAINT asset_records_cost_currency_check CHECK (cost_currency ~ '^[A-Z]{3}$'),
    ticket_id            integer,
    replacement_asset_id integer,
    -- Who held the asset and where it was on occurred_on, and where that
    -- came from: entered, a loan, the asset as it is now, or nothing.
    holder_uuid          uuid         REFERENCES public.users(uuid) ON DELETE SET NULL,
    location             varchar(255),
    snapshot_source      varchar(16)  NOT NULL DEFAULT 'none'
                         CONSTRAINT asset_records_snapshot_source_check
                         CHECK (snapshot_source IN ('entered', 'loan', 'current', 'none')),
    -- Values for the type's field_schema.
    attributes           jsonb        NOT NULL DEFAULT '{}'::jsonb,
    -- Keyed by plugin name; written only through the plugin endpoint.
    plugin_fields        jsonb        NOT NULL DEFAULT '{}'::jsonb,
    -- The transition that opened the record.
    lifecycle_event_id   integer,
    -- The service schedule the record completes; its foreign key comes with
    -- the schedules table.
    schedule_id          integer,
    source               varchar(16)  NOT NULL
                         CONSTRAINT asset_records_source_check
                         CHECK (source IN ('manual', 'api', 'import', 'rule', 'transition',
                                           'schedule', 'backfill', 'plugin')),
    -- The source system's id, for idempotent imports and API writes.
    source_ref           varchar(255),
    -- The CSV import that wrote the record, for Remove. No foreign key:
    -- import jobs never move between workspaces.
    import_job_id        uuid,
    rule_id              integer,
    recorded_at          timestamptz  NOT NULL DEFAULT now(),
    -- Null for system writers, named by source with rule_id or import_job_id.
    recorded_by          uuid         REFERENCES public.users(uuid) ON DELETE SET NULL,
    updated_at           timestamptz  NOT NULL DEFAULT now(),
    updated_by           uuid         REFERENCES public.users(uuid) ON DELETE SET NULL,
    -- Bumped on every update; an edit names the version it read.
    version              integer      NOT NULL DEFAULT 1,
    CONSTRAINT asset_records_asset_id_fkey FOREIGN KEY (workspace_id, asset_id)
        REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE,
    CONSTRAINT asset_records_record_type_id_fkey FOREIGN KEY (workspace_id, record_type_id)
        REFERENCES public.asset_record_types (workspace_id, id) ON DELETE RESTRICT,
    CONSTRAINT asset_records_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id)
        REFERENCES public.tickets (workspace_id, id) ON DELETE SET NULL (ticket_id),
    CONSTRAINT asset_records_replacement_asset_id_fkey FOREIGN KEY (workspace_id, replacement_asset_id)
        REFERENCES public.assets (workspace_id, id) ON DELETE SET NULL (replacement_asset_id),
    CONSTRAINT asset_records_lifecycle_event_id_fkey FOREIGN KEY (workspace_id, lifecycle_event_id)
        REFERENCES public.asset_lifecycle_events (workspace_id, id) ON DELETE SET NULL (lifecycle_event_id),
    CONSTRAINT asset_records_rule_id_fkey FOREIGN KEY (workspace_id, rule_id)
        REFERENCES public.rules (workspace_id, id) ON DELETE SET NULL (rule_id),
    CONSTRAINT asset_records_completed_on_check
        CHECK (completed_on IS NULL OR completed_on >= occurred_on),
    CONSTRAINT asset_records_completed_at_check
        CHECK (completed_at IS NULL OR completed_on IS NOT NULL),
    CONSTRAINT asset_records_outcome_check
        CHECK (outcome IS NULL OR completed_on IS NOT NULL),
    CONSTRAINT asset_records_cost_check
        CHECK ((cost_minor IS NULL) = (cost_currency IS NULL)),
    CONSTRAINT asset_records_replacement_check
        CHECK (replacement_asset_id <> asset_id)
);
ALTER TABLE public.asset_records OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_records_id_seq OWNER TO nosdesk_admin;

CREATE UNIQUE INDEX asset_records_workspace_id_id_key
    ON public.asset_records (workspace_id, id);
CREATE UNIQUE INDEX asset_records_source_ref_key
    ON public.asset_records (workspace_id, source_ref) WHERE source_ref IS NOT NULL;
-- An asset's timeline: date-only records last within their day.
CREATE INDEX asset_records_asset_timeline_idx
    ON public.asset_records (workspace_id, asset_id, occurred_on DESC, occurred_at DESC NULLS LAST, id DESC);
-- Lists and reports across assets by type.
CREATE INDEX asset_records_type_idx
    ON public.asset_records (workspace_id, record_type_id, occurred_on DESC);
CREATE INDEX asset_records_open_idx
    ON public.asset_records (workspace_id, asset_id) WHERE completed_on IS NULL;
CREATE INDEX asset_records_ticket_idx
    ON public.asset_records (workspace_id, ticket_id) WHERE ticket_id IS NOT NULL;
CREATE INDEX asset_records_replacement_idx
    ON public.asset_records (workspace_id, replacement_asset_id) WHERE replacement_asset_id IS NOT NULL;
-- Deleting a lifecycle event clears the records it opened.
CREATE INDEX asset_records_lifecycle_event_idx
    ON public.asset_records (workspace_id, lifecycle_event_id) WHERE lifecycle_event_id IS NOT NULL;
CREATE INDEX asset_records_import_job_idx
    ON public.asset_records (workspace_id, import_job_id) WHERE import_job_id IS NOT NULL;
-- Vendor suggestions.
CREATE INDEX asset_records_vendor_idx
    ON public.asset_records (workspace_id, lower(vendor));

ALTER TABLE public.asset_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.asset_records FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_records_workspace_isolation ON public.asset_records
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

-- The app role may edit only what an edit changes: never the asset, the
-- entry clock and author, or where the record came from. Foreign-key actions
-- still clear the people and link columns, because they run as the owner.
GRANT SELECT, INSERT, DELETE ON TABLE public.asset_records TO nosdesk_app;
GRANT UPDATE (record_type_id, summary, notes, occurred_on, occurred_at, completed_on,
              completed_at, outcome, vendor, performed_by_uuid, reference, cost_minor,
              cost_currency, ticket_id, replacement_asset_id, holder_uuid, location,
              snapshot_source, attributes, plugin_fields, schedule_id, updated_at,
              updated_by, version)
    ON TABLE public.asset_records TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.asset_records_id_seq TO nosdesk_app;

CREATE TRIGGER tr_audit_asset_records AFTER INSERT OR DELETE OR UPDATE ON public.asset_records
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------------
-- A record's files are asset documents that point at it
-- ---------------------------------------------------------------------------

-- A nullable column rewrites no rows, so the asset_media audit trigger
-- doesn't fire. Deleting a record leaves its files on the asset.
ALTER TABLE public.asset_media ADD COLUMN record_id integer;
ALTER TABLE public.asset_media
    ADD CONSTRAINT asset_media_record_id_fkey FOREIGN KEY (workspace_id, record_id)
    REFERENCES public.asset_records (workspace_id, id) ON DELETE SET NULL (record_id);
CREATE INDEX asset_media_record_idx
    ON public.asset_media (workspace_id, record_id) WHERE record_id IS NOT NULL;
