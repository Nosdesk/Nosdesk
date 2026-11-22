-- A ticket's plan (docs/plans/work-planning, slice 1): the time of day of its
-- deadline and of its planned start, a manual order among the jobs of a day,
-- and how long the job takes.
--
-- due_date and start_date stay floating calendar days, stored as midnight
-- UTC; a time is a floating wall-clock time of that day. A time needs its
-- date, and clearing a date clears its time. rank is a fractional index key,
-- compared byte by byte (COLLATE "C") within whatever list is on screen.
ALTER TABLE public.tickets
    ADD COLUMN due_time time,
    ADD COLUMN start_time time,
    ADD COLUMN rank text COLLATE "C",
    ADD COLUMN estimate_minutes integer;

-- A day is a day. A value at UTC midnight is the web client's own write and
-- stays as it is. Any other value (creation's time of day on a recurring
-- ticket with no due date, a REST write with an offset, the demo seed)
-- becomes its UTC calendar day, the day every screen already shows for it.
-- Nobody chose those times, so they are not kept as due_time. As in other
-- data backfills, tr_audit_tickets is off (it needs app.workspace_id, which
-- a migration session doesn't set, NDX01) and so is set_updated_at, so the
-- repair doesn't count as an edit.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;

UPDATE public.tickets
SET due_date = CASE
        WHEN (due_date AT TIME ZONE 'UTC')::time <> '00:00'
        THEN date_trunc('day', due_date AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
        ELSE due_date
    END,
    start_date = CASE
        WHEN (start_date AT TIME ZONE 'UTC')::time <> '00:00'
        THEN date_trunc('day', start_date AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
        ELSE start_date
    END
WHERE (due_date AT TIME ZONE 'UTC')::time <> '00:00'
   OR (start_date AT TIME ZONE 'UTC')::time <> '00:00';

ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;

ALTER TABLE public.tickets
    ADD CONSTRAINT tickets_due_date_is_a_day
        CHECK ((due_date AT TIME ZONE 'UTC')::time = '00:00'),
    ADD CONSTRAINT tickets_start_date_is_a_day
        CHECK ((start_date AT TIME ZONE 'UTC')::time = '00:00'),
    ADD CONSTRAINT tickets_due_time_needs_due_date
        CHECK (due_time IS NULL OR due_date IS NOT NULL),
    ADD CONSTRAINT tickets_start_time_needs_start_date
        CHECK (start_time IS NULL OR start_date IS NOT NULL),
    ADD CONSTRAINT tickets_estimate_minutes_positive
        CHECK (estimate_minutes > 0),
    ADD CONSTRAINT tickets_rank_not_empty
        CHECK (rank <> '');

-- Reordering is not an edit: a write that changes nothing but rank leaves
-- updated_at, which the app reads as "last activity", where it was. Every
-- other change stamps it, as diesel_set_updated_at does.
CREATE FUNCTION public.tickets_set_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (
        NEW IS DISTINCT FROM OLD AND
        NEW.updated_at IS NOT DISTINCT FROM OLD.updated_at AND
        to_jsonb(NEW) - 'rank' IS DISTINCT FROM to_jsonb(OLD) - 'rank'
    ) THEN
        NEW.updated_at := current_timestamp;
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.tickets_set_updated_at() OWNER TO nosdesk_admin;

DROP TRIGGER set_updated_at ON public.tickets;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.tickets_set_updated_at();

-- Nor is it a change the audit log reviews: an update that changes only rank
-- writes no audit row. tr_audit_tickets keeps every other update, so a data
-- backfill still turns off the trigger it always has; inserts and deletes
-- move to their own trigger. tickets has no other AFTER trigger, so nothing
-- fires in a new order.
DROP TRIGGER tr_audit_tickets ON public.tickets;
CREATE TRIGGER tr_audit_tickets AFTER UPDATE ON public.tickets
    FOR EACH ROW
    WHEN ((to_jsonb(old.*) - 'rank') IS DISTINCT FROM (to_jsonb(new.*) - 'rank'))
    EXECUTE FUNCTION public.audit_log_trigger('id');
CREATE TRIGGER tr_audit_tickets_insert_delete AFTER INSERT OR DELETE ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');
