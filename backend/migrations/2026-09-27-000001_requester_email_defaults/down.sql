UPDATE public.notification_types
SET default_channels = '["in_app"]'::jsonb
WHERE code IN ('ticket_created_requester', 'ticket_status_changed');

CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    IF NEW.event_type = 'comment.created'
       OR NEW.event_type = 'ticket_reference.added'
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
