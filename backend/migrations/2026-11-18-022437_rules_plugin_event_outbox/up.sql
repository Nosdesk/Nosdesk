-- Rules can run on a plugin's events (docs/plans/1.2-automation.md, A16).
--
-- The rule event outbox also queues an event a plugin emitted
-- (`POST /api/plugins/{uuid}/events`: aggregate `plugin`, actor kind
-- `plugin`, aggregate id the plugin's uuid, the plugin's payload under
-- `data.event`), but only one whose payload names a ticket by number in
-- `ticket_id`, in a workspace with a live or dry-run `plugin_event` rule on
-- that plugin and event type. The plugin's own lifecycle rows share the
-- aggregate and are written by people, not the plugin, so the actor kind
-- keeps them out. Whether the plugin may read the ticket is the
-- dispatcher's check.
--
-- The ticket events stay as they were, queued only where a rule with a
-- ticket trigger (or a time-elapsed one) can run; a workspace whose only
-- automated rules run on plugin events queues none of them.
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
              AND trigger_kind <> 'plugin_event'
        ) THEN
            INSERT INTO public.rule_event_outbox (sync_id, workspace_id)
            VALUES (NEW.sync_id, NEW.workspace_id)
            ON CONFLICT (sync_id) DO NOTHING;
            PERFORM pg_notify('rule_event_outbox_new', '');
        END IF;
    ELSIF NEW.aggregate = 'plugin'
          AND NEW.actor_kind = 'plugin'
          AND jsonb_typeof(NEW.data->'event'->'ticket_id') = 'number'
    THEN
        IF EXISTS (
            SELECT 1 FROM public.rules
            WHERE workspace_id = NEW.workspace_id
              AND archived_at IS NULL
              AND state IN ('live', 'dry_run')
              AND trigger_kind = 'plugin_event'
              AND trigger_config->>'plugin_uuid' = NEW.aggregate_id
              AND trigger_config->>'event_type' = NEW.event_type
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

-- The trigger's lookup for a plugin event, one probe per event that names a
-- ticket.
CREATE INDEX rules_plugin_event_idx
    ON public.rules (workspace_id, (trigger_config->>'plugin_uuid'))
    WHERE archived_at IS NULL
      AND state IN ('live', 'dry_run')
      AND trigger_kind = 'plugin_event';
