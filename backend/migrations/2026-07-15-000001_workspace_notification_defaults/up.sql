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

-- Workspace isolation, mirroring notification_preferences' policy shape.
ALTER TABLE workspace_notification_defaults ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspace_notification_defaults FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_notification_defaults_workspace_isolation
    ON workspace_notification_defaults
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
