-- Admin-configurable, workspace-wide notification defaults — the missing middle
-- layer of the 3-tier inheritance:
--
--   system default (notification_types.default_channels)
--     -> workspace default (this table; set by a workspace Admin)
--       -> user override (notification_preferences)
--
-- A user inherits the workspace default and may override it, UNLESS the admin
-- marked the cell `locked` (mandatory — e.g. sla_breached emails).
--
-- Unlike notification_preferences (which lacks a natural workspace_id and needs
-- an audit-trigger workaround), this table has a first-class workspace_id, so
-- RLS works naturally.
CREATE TABLE workspace_notification_defaults (
    id serial PRIMARY KEY,
    workspace_id integer NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    notification_type_id integer NOT NULL REFERENCES notification_types(id) ON DELETE CASCADE,
    channel varchar(20) NOT NULL,
    frequency text NOT NULL DEFAULT 'instant'
        CHECK (frequency IN ('instant', 'digest', 'off')),
    locked boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, notification_type_id, channel)
);

CREATE INDEX idx_workspace_notification_defaults_lookup
    ON workspace_notification_defaults (workspace_id, notification_type_id, channel);

-- Ownership + runtime-role grants. A NEW table must set these explicitly:
-- migrations may run as a role other than nosdesk_admin, so the schema-wide
-- `ALTER DEFAULT PRIVILEGES FOR ROLE nosdesk_admin` (which only covers tables
-- CREATED BY nosdesk_admin) does not reach a table created by the migration
-- runner. Without the owner change, nosdesk_admin (the BYPASSRLS role the
-- workspace backup/export runs under) can't see the table via
-- `information_schema` and the export refuses it; without the grant, the
-- RLS-enforced runtime role nosdesk_app can't read/write it. Mirrors the
-- inbound_addresses pattern in the post-v1011 squash.
ALTER TABLE public.workspace_notification_defaults OWNER TO nosdesk_admin;
ALTER SEQUENCE public.workspace_notification_defaults_id_seq OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_notification_defaults TO nosdesk_app;
GRANT ALL ON SEQUENCE public.workspace_notification_defaults_id_seq TO nosdesk_app;

-- Workspace isolation, mirroring notification_preferences' policy shape.
ALTER TABLE workspace_notification_defaults ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_notification_defaults FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_notification_defaults_workspace_isolation
    ON workspace_notification_defaults
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
