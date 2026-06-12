-- Per-workspace staff-seat cap (nullable; NULL = unlimited).
--
-- Set by the control plane on a self-serve trial provision (to 5) and lifted
-- back to NULL when the Stripe subscription activates (trial -> paid). Self-
-- hosted and operator-provisioned workspaces stay NULL (uncapped), so the
-- enforcement path is a no-op for them.
--
-- Only staff memberships (role IN ('owner','admin','agent')) count toward the
-- limit; end-user 'member' (ticket-requester) grants are uncapped.
ALTER TABLE workspaces ADD COLUMN seat_limit INTEGER;

-- Enforce the staff seat cap at the DB layer so it holds across EVERY
-- membership path (add_membership, the workspace_id-defaulting insert in
-- user_helpers, JIT/projection, invitation, role promotion) without each
-- call site having to remember the check.
--
-- Staff = owner/admin/agent; end-user 'member' rows are uncapped. NULL
-- seat_limit = unlimited. Fires BEFORE INSERT (new staff member) and BEFORE
-- UPDATE (a member -> staff promotion consumes a seat; staff -> staff does
-- not). The count excludes the row being written, so re-granting an existing
-- staff member (ON CONFLICT) never false-trips. Raises check_violation with a
-- named constraint the app maps to a 403 (see repository::workspaces::
-- is_seat_limit_violation).
-- SECURITY DEFINER so the trigger's internal counts see ALL rows regardless of
-- the invoking role's RLS. The `user_helpers` insert path runs as the RLS-bound
-- `nosdesk_app` role; without DEFINER the count + seat_limit read could be
-- RLS-filtered and silently under-enforce. Fixed search_path closes the
-- DEFINER search-path injection vector.
CREATE FUNCTION public.enforce_workspace_seat_limit() RETURNS trigger
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, public
    AS $$
DECLARE
    lim INTEGER;
    staff_count INTEGER;
BEGIN
    IF NEW.role NOT IN ('owner', 'admin', 'agent') THEN
        RETURN NEW;
    END IF;
    -- An UPDATE that keeps a staff role consumes no new seat.
    IF TG_OP = 'UPDATE' AND OLD.role IN ('owner', 'admin', 'agent') THEN
        RETURN NEW;
    END IF;
    SELECT seat_limit INTO lim FROM workspaces WHERE id = NEW.workspace_id;
    IF lim IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT count(*) INTO staff_count
        FROM workspace_members
        WHERE workspace_id = NEW.workspace_id
          AND role IN ('owner', 'admin', 'agent')
          AND user_uuid <> NEW.user_uuid;
    IF staff_count >= lim THEN
        RAISE EXCEPTION 'workspace % staff seat limit (%) reached', NEW.workspace_id, lim
            USING ERRCODE = 'check_violation', CONSTRAINT = 'workspace_seat_limit';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER tr_enforce_workspace_seat_limit
    BEFORE INSERT OR UPDATE ON workspace_members
    FOR EACH ROW EXECUTE FUNCTION public.enforce_workspace_seat_limit();
