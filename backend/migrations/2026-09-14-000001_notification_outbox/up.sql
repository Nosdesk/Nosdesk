-- Notifications derived from the event log (docs/plans/notifications-from-sync-actions.md).
--
-- The assignment and mention notifications were attached to the REST handlers,
-- so writes through /api/sync/push and the rich composer produced none. This
-- outbox is the sibling of webhook_outbox: an AFTER INSERT trigger on
-- sync_actions enqueues the rows a deriver can act on, and one dispatcher at a
-- time claims them (FOR UPDATE SKIP LOCKED), derives the notifications and
-- hands them to the existing notify() path.
--
-- Delivery is at least once. A row survives until its notifications are
-- delivered; a failed attempt reschedules it with backoff, and after the last
-- attempt it is kept with dead_at set for inspection rather than deleted.

CREATE TABLE public.notification_outbox (
    sync_id         bigint PRIMARY KEY,
    enqueued_at     timestamptz NOT NULL DEFAULT now(),
    attempts        smallint    NOT NULL DEFAULT 0,
    claimed_at      timestamptz,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    -- An error kind, never a message body.
    last_error      text,
    dead_at         timestamptz
);
ALTER TABLE public.notification_outbox OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.notification_outbox TO nosdesk_app;

CREATE INDEX notification_outbox_due_idx
    ON public.notification_outbox (next_attempt_at)
    WHERE dead_at IS NULL;

-- Only rows the deriver acts on. Ticket emitters put the full row in `data`,
-- so `assignee_uuid` is present on a tag edit too; the `previous_*` keys are
-- set only by the writes that can change the field, which is what makes the
-- IS DISTINCT FROM comparison mean "actually changed". Keying on the pair
-- rather than event_type also catches a combined update, which is labelled by
-- its most specific field and would otherwise mask the assignment.
--
-- ON CONFLICT DO NOTHING for the same reason webhook_outbox_enqueue has it: an
-- occurred_at change across a month boundary re-fires the trigger.
CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    IF NEW.event_type = 'comment.created'
       OR (NEW.data ? 'previous_assignee_uuid'
           AND NEW.data->>'assignee_uuid' IS DISTINCT FROM NEW.data->>'previous_assignee_uuid')
       OR (NEW.data ? 'previous_workflow_state_id'
           AND NEW.data->>'workflow_state_id' IS DISTINCT FROM NEW.data->>'previous_workflow_state_id')
    THEN
        INSERT INTO notification_outbox (sync_id) VALUES (NEW.sync_id)
        ON CONFLICT (sync_id) DO NOTHING;
        PERFORM pg_notify('notification_outbox_new', '');
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.notification_outbox_enqueue() OWNER TO nosdesk_admin;

CREATE TRIGGER tr_sync_actions_notification_outbox
    AFTER INSERT ON public.sync_actions
    FOR EACH ROW EXECUTE FUNCTION public.notification_outbox_enqueue();

-- Idempotency for the retry. A derived notification records the sync action it
-- came from; a second attempt for the same (action, recipient, type) inserts
-- nothing. Handler-path notifications leave it null and are unaffected.
ALTER TABLE public.notifications ADD COLUMN source_sync_id bigint;
CREATE UNIQUE INDEX notifications_source_sync_uq
    ON public.notifications (workspace_id, source_sync_id, user_uuid, notification_type_id)
    WHERE source_sync_id IS NOT NULL;
