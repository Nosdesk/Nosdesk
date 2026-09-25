-- Which of the given users hold a staff seat (owner/admin/agent) in ANY
-- workspace. In hosted mode such a person's identity is owned by their Nosdesk
-- account, so every workspace needs the answer, but workspace_members is
-- workspace-isolated under RLS. This answers only that one question, returning
-- uuids already known to the caller, so request code can ask it without
-- elevating its whole transaction to the BYPASSRLS role.
CREATE FUNCTION public.staff_seat_holders(user_uuids uuid[]) RETURNS SETOF uuid
    LANGUAGE sql STABLE SECURITY DEFINER
    SET search_path = public
    AS $$
    SELECT DISTINCT user_uuid
    FROM workspace_members
    WHERE user_uuid = ANY(user_uuids)
      AND role IN ('owner', 'admin', 'agent')
      AND removed_at IS NULL
$$;

ALTER FUNCTION public.staff_seat_holders(uuid[]) OWNER TO nosdesk_admin;
REVOKE ALL ON FUNCTION public.staff_seat_holders(uuid[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.staff_seat_holders(uuid[]) TO nosdesk_app;
