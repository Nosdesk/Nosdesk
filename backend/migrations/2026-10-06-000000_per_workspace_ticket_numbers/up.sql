-- A ticket's number counts within its workspace: it is what people quote, and
-- what links and email subjects carry. `id` stays the internal key.
--
-- Existing tickets take their id as their number, so every number already
-- quoted (in links, email threads, documents) still finds its ticket; a
-- workspace's new tickets continue from its highest number.

-- Each workspace's numbers come from its own sequence in this schema, created
-- with the workspace. A sequence hands out numbers without holding a lock
-- until the transaction ends, so tickets created at the same time in one
-- workspace never wait on or deadlock with each other; a rolled-back creation
-- leaves a gap, as ids already do.
CREATE SCHEMA ticket_numbers;

ALTER TABLE public.tickets ADD COLUMN number integer;

-- tickets is audited (tr_audit_tickets), and that trigger needs app.workspace_id,
-- which a migration session doesn't set (NDX01). set_updated_at is off too, so
-- the backfill doesn't count as an edit.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;
UPDATE public.tickets SET number = id;
ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;

ALTER TABLE public.tickets ALTER COLUMN number SET NOT NULL;
CREATE UNIQUE INDEX tickets_workspace_number_key ON public.tickets (workspace_id, number);

-- The functions run as their owner (the migration role, which owns the
-- schema), so the app role needs no grant on the sequences.

-- The name of `ws`'s sequence, creating it when missing. Workspaces get theirs
-- up front (trigger below) because one created inside a ticket's transaction
-- makes every other transaction opening a ticket there wait until it commits.
CREATE FUNCTION public.ticket_number_sequence(ws integer) RETURNS text
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
    seq text := format('ticket_numbers.workspace_%s', ws);
BEGIN
    IF to_regclass(seq) IS NULL THEN
        BEGIN
            EXECUTE format('CREATE SEQUENCE IF NOT EXISTS %s', seq);
        EXCEPTION WHEN unique_violation OR duplicate_table THEN
            NULL; -- another transaction created it first
        END;
    END IF;
    RETURN seq;
END;
$$;

-- Move `ws`'s sequence past `n`: a number given explicitly (an import keeping
-- its numbers) or restored from a backup. Serialised per workspace so two such
-- calls can't move it backwards. nextval doesn't take this lock, so explicit
-- numbers assume nothing else is opening tickets in the workspace (an import
-- fills a new one, a restore replaces the instance); if something is, the
-- unique index rejects whichever insert reaches a number second.
CREATE FUNCTION public.advance_ticket_number(ws integer, n integer) RETURNS void
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
    seq text := public.ticket_number_sequence(ws);
    last bigint;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext(seq));
    EXECUTE format('SELECT CASE WHEN is_called THEN last_value ELSE last_value - 1 END FROM %s', seq)
        INTO last;
    IF n > last THEN
        PERFORM setval(seq, n);
    END IF;
END;
$$;

-- Every workspace's sequence set to continue from its highest number, and the
-- sequences of workspaces that no longer exist dropped. Only for when nothing
-- is opening tickets: after the backfill below, and after a backup restore,
-- which loads workspaces and tickets while triggers are off.
CREATE FUNCTION public.sync_ticket_number_sequences() RETURNS void
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
    r record;
BEGIN
    FOR r IN
        SELECT c.relname FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'ticket_numbers' AND c.relkind = 'S'
          AND NOT EXISTS (SELECT 1 FROM public.workspaces w WHERE c.relname = format('workspace_%s', w.id))
    LOOP
        EXECUTE format('DROP SEQUENCE ticket_numbers.%I', r.relname);
    END LOOP;
    FOR r IN
        SELECT w.id, max(t.number) AS top
        FROM public.workspaces w LEFT JOIN public.tickets t ON t.workspace_id = w.id
        GROUP BY w.id
    LOOP
        PERFORM setval(public.ticket_number_sequence(r.id), coalesce(r.top, 1), r.top IS NOT NULL);
    END LOOP;
END;
$$;

REVOKE EXECUTE ON FUNCTION public.ticket_number_sequence(integer) FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION public.advance_ticket_number(integer, integer) FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION public.sync_ticket_number_sequences() FROM PUBLIC;

-- Numbers every new ticket from its workspace's sequence, whichever path
-- creates it. A number given explicitly is kept and the sequence moves past it.
CREATE FUNCTION public.assign_ticket_number() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
BEGIN
    IF NEW.number IS NULL THEN
        NEW.number := nextval(public.ticket_number_sequence(NEW.workspace_id));
    ELSE
        PERFORM public.advance_ticket_number(NEW.workspace_id, NEW.number);
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tickets_assign_number
    BEFORE INSERT ON public.tickets
    FOR EACH ROW EXECUTE FUNCTION public.assign_ticket_number();

-- A workspace's sequence comes and goes with it. A new workspace's starts
-- fresh even if its id was used before (a restore resets the id sequence).
CREATE FUNCTION public.workspace_ticket_numbers() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        EXECUTE format('DROP SEQUENCE IF EXISTS ticket_numbers.workspace_%s', OLD.id);
        RETURN OLD;
    END IF;
    EXECUTE format('DROP SEQUENCE IF EXISTS ticket_numbers.workspace_%s', NEW.id);
    EXECUTE format('CREATE SEQUENCE ticket_numbers.workspace_%s', NEW.id);
    RETURN NEW;
END;
$$;

CREATE TRIGGER workspaces_ticket_numbers
    AFTER INSERT OR DELETE ON public.workspaces
    FOR EACH ROW EXECUTE FUNCTION public.workspace_ticket_numbers();

SELECT public.sync_ticket_number_sequences();
