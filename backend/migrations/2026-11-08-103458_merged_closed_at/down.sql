-- Merged leaves closed_at and closed_by alone again. The stamps the backfill
-- wrote stay: they match the tickets' states.
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
