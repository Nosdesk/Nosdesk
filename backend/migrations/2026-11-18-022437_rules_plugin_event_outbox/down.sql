-- The outbox queues ticket events only again, as the phase 2 schema left it.
DROP INDEX IF EXISTS public.rules_plugin_event_idx;

CREATE OR REPLACE FUNCTION public.rule_event_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path = public, pg_temp
    AS $$
BEGIN
    IF NEW.event_type IN (
           'ticket.created',
           'ticket.updated',
           'ticket.workflow_state_changed',
           'ticket.assignee_changed',
           'ticket.priority_changed',
           'ticket.title_changed',
           'ticket.category_changed',
           'ticket.verification_changed',
           'ticket.resolution_notes_changed',
           'ticket.tags_changed')
       OR (NEW.event_type = 'comment.created' AND NEW.data->>'is_internal' = 'false')
    THEN
        IF EXISTS (
            SELECT 1 FROM public.rules
            WHERE workspace_id = NEW.workspace_id
              AND archived_at IS NULL
              AND state IN ('live', 'dry_run')
              AND trigger_kind <> 'manual'
        ) THEN
            INSERT INTO public.rule_event_outbox (sync_id, workspace_id)
            VALUES (NEW.sync_id, NEW.workspace_id)
            ON CONFLICT (sync_id) DO NOTHING;
            PERFORM pg_notify('rule_event_outbox_new', '');
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.rule_event_outbox_enqueue() OWNER TO nosdesk_admin;
