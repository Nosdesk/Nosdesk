DROP TRIGGER tr_audit_tickets_insert_delete ON public.tickets;
DROP TRIGGER tr_audit_tickets ON public.tickets;
CREATE TRIGGER tr_audit_tickets AFTER INSERT OR DELETE OR UPDATE ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

DROP TRIGGER set_updated_at ON public.tickets;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.diesel_set_updated_at();
DROP FUNCTION public.tickets_set_updated_at();

ALTER TABLE public.tickets
    DROP CONSTRAINT tickets_due_date_is_a_day,
    DROP CONSTRAINT tickets_start_date_is_a_day,
    DROP CONSTRAINT tickets_due_time_needs_due_date,
    DROP CONSTRAINT tickets_start_time_needs_start_date,
    DROP CONSTRAINT tickets_estimate_minutes_positive,
    DROP CONSTRAINT tickets_rank_not_empty;

ALTER TABLE public.tickets
    DROP COLUMN due_time,
    DROP COLUMN start_time,
    DROP COLUMN rank,
    DROP COLUMN estimate_minutes;
