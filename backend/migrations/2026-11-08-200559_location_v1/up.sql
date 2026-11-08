-- Locations (docs/plans/location-first-party-handoff.md, PR 2).
--
-- Coordinates on a person's addresses, and a location on tickets and assets.
-- Nothing reads or writes these until the `location_v1` flag is on.
--
-- Geocode columns, on all three tables:
--   geocode_precision        house, street or locality: how close the
--                            geocoder got. NULL for a pin dropped by hand.
--   geocode_source           the dataset a result came from, or 'manual'
--                            for a pin someone placed.
--   geocode_place_confirmed  false when the match sits in a different
--                            suburb or postcode from the one asked for.
-- Anything coarser than a house, or not confirmed, shows as approximate.

ALTER TABLE public.user_addresses
    ADD COLUMN latitude                double precision,
    ADD COLUMN longitude               double precision,
    ADD COLUMN geocode_precision       varchar(16),
    ADD COLUMN geocode_source          varchar(64),
    ADD COLUMN geocode_place_confirmed boolean,
    ADD COLUMN geocoded_at             timestamptz,
    -- Hash of the normalised address text when it was last geocoded. A
    -- mismatch means the text changed, by an edit or a directory sync, and
    -- the coordinates are stale. A match with no coordinates means the
    -- geocoder found nothing for this text, so it isn't retried.
    ADD COLUMN geocoded_hash           varchar(64),
    ADD CONSTRAINT user_addresses_coordinates_check CHECK (
        (latitude IS NULL) = (longitude IS NULL)
        AND (latitude IS NULL OR latitude BETWEEN -90 AND 90)
        AND (longitude IS NULL OR longitude BETWEEN -180 AND 180)
    ),
    ADD CONSTRAINT user_addresses_geocode_precision_check CHECK (
        geocode_precision IN ('house', 'street', 'locality')
    );

-- A ticket's location is a copy, never a reference: choosing the requester's
-- address copies it, so the ticket keeps where the job was when they move.
-- `origin_type` is the copied address's type (work, home, other), NULL when
-- an agent searched or dropped a pin. Erasure reads it.
-- Writes refuse an empty location (repository::locations::normalise), but the
-- table doesn't, so erasure is free to coarsen a row down to almost nothing.
CREATE TABLE public.ticket_locations (
    ticket_id               integer      PRIMARY KEY,
    label                   varchar(100),
    street                  varchar(255),
    city                    varchar(128),
    region                  varchar(128),
    postal_code             varchar(32),
    country                 varchar(128),
    latitude                double precision,
    longitude               double precision,
    geocode_precision       varchar(16),
    geocode_source          varchar(64),
    geocode_place_confirmed boolean,
    origin_type             varchar(16),
    created_by              uuid         REFERENCES public.users(uuid) ON DELETE SET NULL,
    workspace_id            integer      NOT NULL
                            DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                            REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at              timestamptz  NOT NULL DEFAULT now(),
    updated_at              timestamptz  NOT NULL DEFAULT now(),
    -- With the workspace, so a location can't point at another workspace's ticket.
    CONSTRAINT ticket_locations_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id)
        REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE,
    CONSTRAINT ticket_locations_coordinates_check CHECK (
        (latitude IS NULL) = (longitude IS NULL)
        AND (latitude IS NULL OR latitude BETWEEN -90 AND 90)
        AND (longitude IS NULL OR longitude BETWEEN -180 AND 180)
    ),
    CONSTRAINT ticket_locations_geocode_precision_check CHECK (
        geocode_precision IN ('house', 'street', 'locality')
    ),
    CONSTRAINT ticket_locations_origin_type_check CHECK (
        origin_type IN ('work', 'home', 'other')
    )
);
ALTER TABLE public.ticket_locations OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.ticket_locations TO nosdesk_app;

CREATE INDEX ticket_locations_workspace_idx ON public.ticket_locations (workspace_id);
CREATE INDEX ticket_locations_created_by_idx ON public.ticket_locations (created_by);

ALTER TABLE public.ticket_locations ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_locations FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_locations_workspace_isolation ON public.ticket_locations
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

-- The audit trail records who changed a location and when, and which parts
-- changed, never the address or the coordinates.
CREATE TRIGGER tr_audit_ticket_locations AFTER INSERT OR DELETE OR UPDATE ON public.ticket_locations
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'ticket_id', 'label', 'street', 'city', 'region', 'postal_code', 'country',
        'latitude', 'longitude');

-- An asset's structured location. `assets.location`, the free text, stays as
-- it is and reads as a note beside this.
CREATE TABLE public.asset_locations (
    asset_id                integer      PRIMARY KEY,
    label                   varchar(100),
    street                  varchar(255),
    city                    varchar(128),
    region                  varchar(128),
    postal_code             varchar(32),
    country                 varchar(128),
    latitude                double precision,
    longitude               double precision,
    geocode_precision       varchar(16),
    geocode_source          varchar(64),
    geocode_place_confirmed boolean,
    origin_type             varchar(16),
    created_by              uuid         REFERENCES public.users(uuid) ON DELETE SET NULL,
    workspace_id            integer      NOT NULL
                            DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                            REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at              timestamptz  NOT NULL DEFAULT now(),
    updated_at              timestamptz  NOT NULL DEFAULT now(),
    CONSTRAINT asset_locations_asset_id_fkey FOREIGN KEY (workspace_id, asset_id)
        REFERENCES public.assets (workspace_id, id) ON DELETE CASCADE,
    CONSTRAINT asset_locations_coordinates_check CHECK (
        (latitude IS NULL) = (longitude IS NULL)
        AND (latitude IS NULL OR latitude BETWEEN -90 AND 90)
        AND (longitude IS NULL OR longitude BETWEEN -180 AND 180)
    ),
    CONSTRAINT asset_locations_geocode_precision_check CHECK (
        geocode_precision IN ('house', 'street', 'locality')
    ),
    CONSTRAINT asset_locations_origin_type_check CHECK (
        origin_type IN ('work', 'home', 'other')
    )
);
ALTER TABLE public.asset_locations OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.asset_locations TO nosdesk_app;

CREATE INDEX asset_locations_workspace_idx ON public.asset_locations (workspace_id);
CREATE INDEX asset_locations_created_by_idx ON public.asset_locations (created_by);

ALTER TABLE public.asset_locations ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.asset_locations FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_locations_workspace_isolation ON public.asset_locations
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

CREATE TRIGGER tr_audit_asset_locations AFTER INSERT OR DELETE OR UPDATE ON public.asset_locations
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger(
        'asset_id', 'label', 'street', 'city', 'region', 'postal_code', 'country',
        'latitude', 'longitude');

-- Sync records name only the ticket or asset, never the address, so the sync
-- stream holds no location to erase. Added here, first used by a later emit.
ALTER TYPE public.sync_aggregate ADD VALUE IF NOT EXISTS 'ticket_location';
ALTER TYPE public.sync_aggregate ADD VALUE IF NOT EXISTS 'asset_location';
