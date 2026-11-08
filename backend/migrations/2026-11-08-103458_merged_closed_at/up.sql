-- A merged ticket is closed. ticket_closed_follows_state left Merged alone,
-- expecting the merge to stamp closed_at, and the merge never did, so a ticket
-- merged while open stayed open on every count that reads closed_at.
--
-- Merged now stamps like Done and Cancelled: entering it stamps the time and
-- the acting user (app.actor_uuid, when it names a user), and a ticket that
-- arrives already stamped (one closed before its merge) keeps its stamp. A
-- statement that sets closed_at itself keeps its value. Any other state
-- clears both.
CREATE OR REPLACE FUNCTION public.ticket_closed_follows_state() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    new_category public.workflow_state_category;
    old_category public.workflow_state_category;
    actor UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
    closer UUID;
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.workflow_state_id IS NOT DISTINCT FROM OLD.workflow_state_id THEN
        RETURN NEW;
    END IF;

    SELECT category INTO new_category
    FROM public.workflow_states WHERE id = NEW.workflow_state_id;

    -- An unreadable state decides nothing.
    IF new_category IS NULL THEN
        RETURN NEW;
    END IF;

    IF new_category NOT IN ('done', 'cancelled', 'merged') THEN
        NEW.closed_at := NULL;
        NEW.closed_by := NULL;
        RETURN NEW;
    END IF;

    IF TG_OP = 'UPDATE' THEN
        IF NEW.closed_at IS DISTINCT FROM OLD.closed_at THEN
            RETURN NEW;
        END IF;
        SELECT category INTO old_category
        FROM public.workflow_states WHERE id = OLD.workflow_state_id;
        IF old_category IN ('done', 'cancelled', 'merged') AND OLD.closed_at IS NOT NULL THEN
            RETURN NEW;
        END IF;
    ELSIF NEW.closed_at IS NOT NULL THEN
        RETURN NEW;
    END IF;

    SELECT u.uuid INTO closer FROM public.users u WHERE u.uuid = actor;
    NEW.closed_at := now();
    NEW.closed_by := closer;
    RETURN NEW;
END;
$$;

-- Stamp the tickets merged while open: closed when merged (never before the
-- ticket was created), by whoever merged it, when that user still exists. As
-- in other data backfills, tr_audit_tickets is off (it needs app.workspace_id,
-- which a migration session doesn't set, NDX01) and so is set_updated_at, so
-- the repair doesn't count as an edit.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;

UPDATE public.tickets t
SET closed_at = GREATEST(m.merged_at, t.created_at),
    closed_by = (SELECT u.uuid FROM public.users u WHERE u.uuid = m.merged_by_user_uuid)
FROM public.ticket_merges m, public.workflow_states ws
WHERE m.ticket_id = t.id
  AND ws.id = t.workflow_state_id
  AND ws.category = 'merged'
  AND t.closed_at IS NULL;

ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;
