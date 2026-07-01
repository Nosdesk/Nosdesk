-- v1.1 asset lifecycle + export, increment 1 (schema).
--
-- The two new statuses (on_order, in_transit) are app-level only: assets.status
-- is a bare varchar(32) with no CHECK constraint, so the AssetStatus enum is the
-- sole gate and needs no DB change here. This migration adds the two-axis-custody
-- column and the disposal-record table.

-- Two-axis custody: the accountable "managed by", distinct from the holder
-- (primary_user_uuid, "used by"). Nullable; matches the industry norm
-- (ServiceNow assigned_to/managed_by, Freshservice Used By/Managed By).
ALTER TABLE assets
    ADD COLUMN managed_by_user_uuid uuid REFERENCES users(uuid) ON DELETE SET NULL;

-- Disposal record (NIST SP 800-88 aligned), captured on the `disposed`
-- transition for compliance / chain-of-custody export. One row per disposal,
-- linked to the disposed lifecycle event. Written once (append-only in practice).
CREATE TABLE asset_disposals (
    id serial PRIMARY KEY,
    asset_id integer NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    lifecycle_event_id integer REFERENCES asset_lifecycle_events(id) ON DELETE SET NULL,
    -- NIST 800-88 sanitization category: clear | purge | destroy | none.
    sanitization_method varchar(16) NOT NULL,
    -- Did the device hold data? Drives the certificate requirement in the app.
    data_bearing boolean NOT NULL DEFAULT true,
    -- Certificate of sanitization/destruction. Soft file reference for now; the
    -- compliance pack (v1.2) wires the attachment fully.
    certificate_file_id integer,
    itad_vendor text,
    notes text,
    actor_uuid uuid REFERENCES users(uuid) ON DELETE SET NULL,
    occurred_at timestamptz NOT NULL DEFAULT now(),
    workspace_id integer NOT NULL
        DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
        REFERENCES workspaces(id)
);

ALTER TABLE asset_disposals OWNER TO nosdesk_admin;
ALTER TABLE asset_disposals ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_disposals FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_disposals_workspace_isolation ON asset_disposals
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE asset_disposals TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE asset_disposals_id_seq TO nosdesk_app;

CREATE INDEX asset_disposals_asset_id_idx ON asset_disposals (asset_id);
