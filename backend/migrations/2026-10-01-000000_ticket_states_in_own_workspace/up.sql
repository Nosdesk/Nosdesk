-- Move tickets that hold another workspace's workflow state to the matching
-- state in their own workspace: the same category first, then the default,
-- then the lowest position.
--
-- A process-wide cache of workflow states, removed in the same release, could
-- give a new ticket the default state of whichever workspace on the server
-- read its states first. Single-workspace installs have nothing to repair.
--
-- tickets is audited (tr_audit_tickets), and that trigger needs
-- app.workspace_id, which a migration session doesn't set (NDX01), so it is off
-- for the repair, as in other data backfills.
ALTER TABLE public.tickets DISABLE TRIGGER tr_audit_tickets;

UPDATE public.tickets t
SET workflow_state_id = (
    SELECT own.id
    FROM public.workflow_states own
    WHERE own.workspace_id = t.workspace_id
      AND own.archived_at IS NULL
    ORDER BY (own.category = foreign_state.category) DESC, own.is_default DESC, own.position
    LIMIT 1
)
FROM public.workflow_states foreign_state
WHERE foreign_state.id = t.workflow_state_id
  AND foreign_state.workspace_id <> t.workspace_id
  AND EXISTS (
      SELECT 1
      FROM public.workflow_states own
      WHERE own.workspace_id = t.workspace_id
        AND own.archived_at IS NULL
  );

ALTER TABLE public.tickets ENABLE TRIGGER tr_audit_tickets;
