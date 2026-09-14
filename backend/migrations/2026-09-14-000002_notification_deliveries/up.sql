-- Per-channel delivery state for notifications
-- (docs/plans/notifications-from-sync-actions.md, step 3).
--
-- `notifications.channels_delivered` is a list of channels that succeeded and
-- nothing else: a push the relay refused, or an email the queue rejected, left
-- no record and was never retried. One row per notification x channel gives
-- the retry worker something to act on, and is the primitive the escalation
-- ladder (deferred) builds on. The JSONB list stays as the read model the API
-- already serves; both are written together.

CREATE TABLE public.notification_deliveries (
    notification_id integer     NOT NULL REFERENCES public.notifications(id) ON DELETE CASCADE,
    channel         varchar(16) NOT NULL,
    -- pending | delivered | failed | skipped
    status          varchar(16) NOT NULL DEFAULT 'pending',
    attempts        smallint    NOT NULL DEFAULT 0,
    next_attempt_at timestamptz,
    -- An error kind, never a message body.
    last_error      text,
    delivered_at    timestamptz,
    -- The payload the channel needs, so a retry does not rebuild it from the
    -- notification row (which has no actor and a flattened entity).
    payload         jsonb       NOT NULL,
    workspace_id    integer     NOT NULL REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (notification_id, channel)
);
ALTER TABLE public.notification_deliveries OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.notification_deliveries TO nosdesk_app;

-- What the retry worker scans.
CREATE INDEX notification_deliveries_due_idx
    ON public.notification_deliveries (next_attempt_at)
    WHERE status = 'pending' AND next_attempt_at IS NOT NULL;

ALTER TABLE public.notification_deliveries ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.notification_deliveries FORCE ROW LEVEL SECURITY;
CREATE POLICY notification_deliveries_workspace_isolation ON public.notification_deliveries
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
