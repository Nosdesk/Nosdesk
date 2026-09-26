-- A requester's own ticket events email them by default: the acknowledgement
-- when their request is opened, and a change of status (e.g. resolved). Both
-- types are only ever addressed to the ticket's requester. Reply
-- notifications stay in-app: the reply itself already reaches them by email.
-- A workspace default or a user preference still overrides this.
UPDATE public.notification_types
SET default_channels = '["in_app", "email"]'::jsonb
WHERE code IN ('ticket_created_requester', 'ticket_status_changed');

-- The outbox trigger also enqueues a new ticket with a requester, so the
-- deriver can acknowledge it. Same body as 2026-09-14-000003 plus that
-- predicate.
CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    IF NEW.event_type = 'comment.created'
       OR NEW.event_type = 'ticket_reference.added'
       OR (NEW.event_type = 'ticket.created' AND NEW.data->>'requester_uuid' IS NOT NULL)
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
