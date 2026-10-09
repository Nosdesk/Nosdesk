-- With organisation sharing on (portal_share_by_domain), requests that staff
-- raised under their own name are shared only when portal_share_staff_requests
-- is on. Off by default. Adding a column with a constant default rewrites no
-- rows, so the site_settings audit trigger doesn't fire.
ALTER TABLE public.site_settings
    ADD COLUMN portal_share_staff_requests boolean NOT NULL DEFAULT false;

-- Whether the ticket's requester was staff when it was raised: an agent, admin
-- or owner of the ticket's workspace, or a platform admin. A fact about the
-- ticket, so a later promotion, demotion or removal doesn't change it.
ALTER TABLE public.tickets
    ADD COLUMN raised_by_staff boolean NOT NULL DEFAULT false;

-- Set on every write path: on insert, and whenever the requester changes, from
-- the requester's role at that moment. Any other update keeps the value, so
-- nothing rewrites it. A workspace import (nosdesk.in_audit_read on) inserts
-- the exported value as it is, whatever order its tables load in.
CREATE FUNCTION public.ticket_raised_by_staff() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    is_staff boolean;
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.requester_uuid IS NOT DISTINCT FROM OLD.requester_uuid THEN
        NEW.raised_by_staff := OLD.raised_by_staff;
        RETURN NEW;
    END IF;
    IF TG_OP = 'INSERT' AND current_setting('nosdesk.in_audit_read', true) = 'true' THEN
        RETURN NEW;
    END IF;

    is_staff := NEW.requester_uuid IS NOT NULL AND (
        EXISTS (
            SELECT 1 FROM public.workspace_members m
            WHERE m.workspace_id = NEW.workspace_id
              AND m.user_uuid = NEW.requester_uuid
              AND m.removed_at IS NULL
              AND m.role IN ('agent', 'admin', 'owner'))
        OR EXISTS (
            SELECT 1 FROM public.users u
            WHERE u.uuid = NEW.requester_uuid
              AND u.platform_role = 'platform_admin'));

    NEW.raised_by_staff := is_staff;
    RETURN NEW;
END;
$$;

-- Existing tickets: the requester's role now, the nearest record there is.
-- Staff who have since been removed still count, so their requests stay
-- unshared. tickets is audited (tr_audit_tickets), and that trigger needs
-- app.workspace_id, which a migration session doesn't set (NDX01), so it is
-- off for the backfill, as in other data backfills; set_updated_at is off
-- too, so the backfill doesn't count as an edit.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;

UPDATE public.tickets t
SET raised_by_staff = true
WHERE EXISTS (
        SELECT 1 FROM public.workspace_members m
        WHERE m.workspace_id = t.workspace_id
          AND m.user_uuid = t.requester_uuid
          AND m.role IN ('agent', 'admin', 'owner'))
   OR EXISTS (
        SELECT 1 FROM public.users u
        WHERE u.uuid = t.requester_uuid
          AND u.platform_role = 'platform_admin');

ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;

CREATE TRIGGER ticket_raised_by_staff
    BEFORE INSERT OR UPDATE ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.ticket_raised_by_staff();
