-- Registered devices for mobile/web push. A user may have several. The push
-- channel loads a recipient's ACTIVE tokens (revoked_at IS NULL) and sends a
-- MINIMAL payload (notification type + entity ref only — no customer content;
-- the app fetches details after the tap), so Apple/Google never see ticket
-- text. See docs/notification-preferences-and-push-design-2026-07-15.md.
CREATE TABLE user_push_devices (
    id serial PRIMARY KEY,
    user_uuid uuid NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    workspace_id integer NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    platform varchar(16) NOT NULL CHECK (platform IN ('ios', 'android', 'web')),
    token text NOT NULL,
    app_version text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    -- One row per device token; re-registering the same token (reinstall, or a
    -- token reassigned to another user) upserts onto this key.
    UNIQUE (token)
);

CREATE INDEX idx_user_push_devices_active
    ON user_push_devices (user_uuid)
    WHERE revoked_at IS NULL;

-- Ownership + runtime-role grants. New tables MUST set these (see
-- 2026-07-15-000001): without them nosdesk_app can't see the table and the
-- workspace backup/export refuses it.
ALTER TABLE public.user_push_devices OWNER TO nosdesk_admin;
ALTER SEQUENCE public.user_push_devices_id_seq OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.user_push_devices TO nosdesk_app;
GRANT ALL ON SEQUENCE public.user_push_devices_id_seq TO nosdesk_app;

-- Workspace isolation.
ALTER TABLE public.user_push_devices ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.user_push_devices FORCE ROW LEVEL SECURITY;
CREATE POLICY user_push_devices_workspace_isolation
    ON user_push_devices
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
