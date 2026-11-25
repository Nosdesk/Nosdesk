-- Named asset statuses (asset foundations B2): a workspace's own names for
-- its assets' statuses, each inside one of the nine fixed categories that
-- `assets.status` keeps holding.
--
-- A null `status_id` is the plain category: it reads as the category's
-- default named status when the workspace has one, else as the catalogue
-- name. Nothing is backfilled and no rows are seeded, so every existing
-- asset, event and loan stays plain and the new table starts empty.
--
-- Each `status_id` column has two foreign keys. `(workspace_id, x_id)` to
-- `(workspace_id, id)` is the one the workspace archive discovers and
-- remaps; `(workspace_id, <category column>, x_id)` to
-- `(workspace_id, category, id)` makes a named status of another category
-- impossible. Both are NO ACTION: a used status is archived, not deleted,
-- and the workspace delete cascade removes both sides in one statement.
-- Adding the nullable columns rewrites no rows, so no audit trigger fires.

CREATE TABLE public.asset_statuses (
    id               serial      PRIMARY KEY,
    workspace_id     integer     NOT NULL
                     DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                     REFERENCES public.workspaces(id) ON DELETE CASCADE,
    -- One of the nine `AssetStatus` keys, app-validated as `assets.status`
    -- is. Never changes once the status exists.
    category         varchar(32) NOT NULL,
    -- Stable key for the API, imports and plugins; never changes.
    slug             varchar(64) NOT NULL
                     CONSTRAINT asset_statuses_slug_check CHECK (slug ~ '^[a-z][a-z0-9_]*$'),
    label            varchar(64) NOT NULL,
    description      text,
    -- Null uses the category's color.
    color            varchar(7)
                     CONSTRAINT asset_statuses_color_check CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    -- Order within the category.
    position         integer     NOT NULL DEFAULT 0,
    -- The category's name in this workspace: plain assets of every kind
    -- read as it.
    is_default       boolean     NOT NULL DEFAULT false,
    -- Asset kind slugs offered this status; null means every kind.
    applies_to_kinds text[],
    -- Archived statuses stay on the rows that hold them and leave pickers.
    archived_at      timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    created_by       uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    CONSTRAINT asset_statuses_default_kinds_check
        CHECK (NOT is_default OR applies_to_kinds IS NULL)
);
ALTER TABLE public.asset_statuses OWNER TO nosdesk_admin;
ALTER SEQUENCE public.asset_statuses_id_seq OWNER TO nosdesk_admin;

CREATE UNIQUE INDEX asset_statuses_workspace_id_id_key
    ON public.asset_statuses (workspace_id, id);
CREATE UNIQUE INDEX asset_statuses_workspace_category_id_key
    ON public.asset_statuses (workspace_id, category, id);
CREATE UNIQUE INDEX asset_statuses_workspace_slug_key
    ON public.asset_statuses (workspace_id, slug);
-- A label names one live status, so an import cell or API value does.
CREATE UNIQUE INDEX asset_statuses_label_key
    ON public.asset_statuses (workspace_id, lower(label))
    WHERE archived_at IS NULL;
-- One live default per category.
CREATE UNIQUE INDEX asset_statuses_default_key
    ON public.asset_statuses (workspace_id, category)
    WHERE is_default AND archived_at IS NULL;

ALTER TABLE public.asset_statuses ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.asset_statuses FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_statuses_workspace_isolation ON public.asset_statuses
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.asset_statuses TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.asset_statuses_id_seq TO nosdesk_app;

CREATE TRIGGER tr_audit_asset_statuses AFTER INSERT OR DELETE OR UPDATE ON public.asset_statuses
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- ---------------------------------------------------------------------------
-- The named status on an asset, its events and its loans
-- ---------------------------------------------------------------------------

ALTER TABLE public.assets ADD COLUMN status_id integer;
ALTER TABLE public.assets
    ADD CONSTRAINT assets_status_id_fkey FOREIGN KEY (workspace_id, status_id)
    REFERENCES public.asset_statuses (workspace_id, id);
ALTER TABLE public.assets
    ADD CONSTRAINT assets_status_category_fkey FOREIGN KEY (workspace_id, status, status_id)
    REFERENCES public.asset_statuses (workspace_id, category, id);
CREATE INDEX assets_status_id_idx
    ON public.assets (workspace_id, status_id) WHERE status_id IS NOT NULL;

ALTER TABLE public.asset_lifecycle_events
    ADD COLUMN from_status_id integer,
    ADD COLUMN to_status_id integer;
ALTER TABLE public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_from_status_id_fkey
    FOREIGN KEY (workspace_id, from_status_id)
    REFERENCES public.asset_statuses (workspace_id, id);
ALTER TABLE public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_from_status_category_fkey
    FOREIGN KEY (workspace_id, from_status, from_status_id)
    REFERENCES public.asset_statuses (workspace_id, category, id);
ALTER TABLE public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_to_status_id_fkey
    FOREIGN KEY (workspace_id, to_status_id)
    REFERENCES public.asset_statuses (workspace_id, id);
ALTER TABLE public.asset_lifecycle_events
    ADD CONSTRAINT asset_lifecycle_events_to_status_category_fkey
    FOREIGN KEY (workspace_id, to_status, to_status_id)
    REFERENCES public.asset_statuses (workspace_id, category, id);
CREATE INDEX asset_lifecycle_events_from_status_id_idx
    ON public.asset_lifecycle_events (workspace_id, from_status_id)
    WHERE from_status_id IS NOT NULL;
CREATE INDEX asset_lifecycle_events_to_status_id_idx
    ON public.asset_lifecycle_events (workspace_id, to_status_id)
    WHERE to_status_id IS NOT NULL;

-- The named status a return puts back, beside status_before.
ALTER TABLE public.asset_loans ADD COLUMN status_id_before integer;
ALTER TABLE public.asset_loans
    ADD CONSTRAINT asset_loans_status_id_before_fkey
    FOREIGN KEY (workspace_id, status_id_before)
    REFERENCES public.asset_statuses (workspace_id, id);
ALTER TABLE public.asset_loans
    ADD CONSTRAINT asset_loans_status_before_category_fkey
    FOREIGN KEY (workspace_id, status_before, status_id_before)
    REFERENCES public.asset_statuses (workspace_id, category, id);
CREATE INDEX asset_loans_status_id_before_idx
    ON public.asset_loans (workspace_id, status_id_before)
    WHERE status_id_before IS NOT NULL;
