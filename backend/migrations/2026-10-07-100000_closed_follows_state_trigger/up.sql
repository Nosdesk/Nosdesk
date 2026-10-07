-- A ticket's closed_at and closed_by follow its workflow state on every write
-- path. Only the REST PATCH and sync push derived closed_at in the app, so a
-- rule, quick-add or a ticket created already closed was left unstamped, and
-- closed_by was never written at all.
--
-- Entering a Done or Cancelled state stamps the time and the acting user
-- (app.actor_uuid, when it names a user). Moving between those states keeps
-- the first stamp. A statement that sets closed_at itself (an import, a
-- restore, a backdated seed) keeps its value. Merged leaves both alone. Any
-- other state clears both.
CREATE FUNCTION public.ticket_closed_follows_state() RETURNS trigger
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

    -- An unreadable state decides nothing; Merged keeps what the merge left.
    IF new_category IS NULL OR new_category = 'merged' THEN
        RETURN NEW;
    END IF;

    IF new_category NOT IN ('done', 'cancelled') THEN
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
        IF old_category IN ('done', 'cancelled') AND OLD.closed_at IS NOT NULL THEN
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

-- Repair the rows the app paths missed. tickets is audited (tr_audit_tickets),
-- and that trigger needs app.workspace_id, which a migration session doesn't
-- set (NDX01), so it is off for the repair, as in other data backfills;
-- set_updated_at is off too, so the repair doesn't count as an edit.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;

-- Closed but unstamped: the time of the last move into the current state, or
-- the last update when no move was recorded.
UPDATE public.tickets t
SET closed_at = COALESCE(
    (SELECT max(sa.occurred_at)
     FROM public.sync_actions sa
     WHERE sa.aggregate = 'ticket'
       AND sa.aggregate_id = t.id::text
       AND sa.workspace_id = t.workspace_id
       AND sa.event_type = 'ticket.workflow_state_changed'
       AND sa.data->>'workflow_state_id' = t.workflow_state_id::text),
    t.updated_at)
FROM public.workflow_states ws
WHERE ws.id = t.workflow_state_id
  AND ws.category IN ('done', 'cancelled')
  AND t.closed_at IS NULL;

-- closed_by was never written: the user behind the last move into the current
-- state, when there is one.
UPDATE public.tickets t
SET closed_by = (
    SELECT sa.actor_uuid
    FROM public.sync_actions sa
    JOIN public.users u ON u.uuid = sa.actor_uuid
    WHERE sa.aggregate = 'ticket'
      AND sa.aggregate_id = t.id::text
      AND sa.workspace_id = t.workspace_id
      AND sa.event_type = 'ticket.workflow_state_changed'
      AND sa.data->>'workflow_state_id' = t.workflow_state_id::text
    ORDER BY sa.occurred_at DESC
    LIMIT 1)
FROM public.workflow_states ws
WHERE ws.id = t.workflow_state_id
  AND ws.category IN ('done', 'cancelled')
  AND t.closed_by IS NULL;

-- Open tickets carry neither.
UPDATE public.tickets t
SET closed_at = NULL, closed_by = NULL
FROM public.workflow_states ws
WHERE ws.id = t.workflow_state_id
  AND ws.category NOT IN ('done', 'cancelled', 'merged')
  AND (t.closed_at IS NOT NULL OR t.closed_by IS NOT NULL);

ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;

CREATE TRIGGER ticket_closed_follows_state
    BEFORE INSERT OR UPDATE OF workflow_state_id ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.ticket_closed_follows_state();
