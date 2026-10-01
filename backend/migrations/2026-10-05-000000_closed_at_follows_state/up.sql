-- A ticket moved between open and closed states through sync push kept its
-- old closed_at: only the REST PATCH set it from the state. Close each closed
-- ticket at its last move into its current state (or its last update, when no
-- move was recorded), and clear it on open tickets. Merged tickets keep what
-- the merge set.
--
-- tickets is audited (tr_audit_tickets), and that trigger needs
-- app.workspace_id, which a migration session doesn't set (NDX01), so it is off
-- for the repair, as in other data backfills. set_updated_at is off too, so the
-- repair doesn't count as an edit.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;
ALTER TABLE public.tickets DISABLE TRIGGER set_updated_at;

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

UPDATE public.tickets t
SET closed_at = NULL
FROM public.workflow_states ws
WHERE ws.id = t.workflow_state_id
  AND ws.category NOT IN ('done', 'cancelled', 'merged')
  AND t.closed_at IS NOT NULL;

ALTER TABLE public.tickets ENABLE TRIGGER set_updated_at;
ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;
