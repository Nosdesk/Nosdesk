-- Restore the three outbox functions to their previous bodies (without the
-- import early return), drop the id map, and narrow the job types. The rows a
-- helpdesk import wrote stay; only its bookkeeping goes.

CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    IF NEW.event_type = 'comment.created'
       OR NEW.event_type = 'ticket_reference.added'
       OR NEW.event_type IN ('ticket.approval_requested', 'ticket.approval_decided')
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

CREATE OR REPLACE FUNCTION public.webhook_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    INSERT INTO webhook_outbox (sync_id) VALUES (NEW.sync_id)
    ON CONFLICT (sync_id) DO NOTHING;
    RETURN NEW;
END;
$$;

DROP TABLE public.import_source_ids;

-- Helpdesk jobs can't exist under the old constraint. The audit trigger needs
-- a workspace context a migration doesn't have.
ALTER TABLE public.import_jobs DISABLE TRIGGER tr_audit_import_jobs;
DELETE FROM public.import_jobs WHERE job_type = 'helpdesk';
ALTER TABLE public.import_jobs ENABLE TRIGGER tr_audit_import_jobs;

ALTER TABLE public.import_jobs DROP CONSTRAINT import_jobs_job_type_check;
ALTER TABLE public.import_jobs ADD CONSTRAINT import_jobs_job_type_check
    CHECK (job_type IN ('assets', 'users', 'tickets', 'documentation', 'asset_records'));
