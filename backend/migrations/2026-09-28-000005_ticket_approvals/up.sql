-- Approvals, the request flow (docs/plans/requester-phase3.md). A request in a
-- type that needs approval waits until its approvers decide.

-- Where a ticket stands: NULL (no approval involved), pending, approved,
-- declined or skipped. Denormalised from ticket_approvals so queues can filter
-- and badge without a join.
ALTER TABLE public.tickets
    ADD COLUMN approval_state text
        CHECK (approval_state IN ('pending', 'approved', 'declined', 'skipped'));

-- One row per approver per approval round. `decision` NULL = still waiting.
CREATE TABLE public.ticket_approvals (
    id           serial      PRIMARY KEY,
    ticket_id    integer     NOT NULL REFERENCES public.tickets(id) ON DELETE CASCADE,
    approver_uuid uuid       NOT NULL REFERENCES public.users(uuid) ON DELETE CASCADE,
    decision     text        CHECK (decision IN ('approved', 'declined', 'skipped')),
    comment      text,
    -- How it was decided: portal, email, app, self (the approver is the
    -- requester), skip (staff) or timeout.
    channel      text,
    decided_by   uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    decided_at   timestamptz,
    -- A reopened declined request starts a new round; old rows stay as history.
    round        integer     NOT NULL DEFAULT 1,
    workspace_id integer     NOT NULL
                 DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                 REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (ticket_id, round, approver_uuid)
);
ALTER TABLE public.ticket_approvals OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.ticket_approvals TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.ticket_approvals_id_seq TO nosdesk_app;
CREATE INDEX ticket_approvals_ticket_idx ON public.ticket_approvals (ticket_id);
CREATE INDEX ticket_approvals_waiting_idx ON public.ticket_approvals (workspace_id, approver_uuid)
    WHERE decision IS NULL;
ALTER TABLE public.ticket_approvals ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_approvals FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_approvals_workspace_isolation ON public.ticket_approvals
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
CREATE TRIGGER tr_audit_ticket_approvals AFTER INSERT OR DELETE OR UPDATE ON public.ticket_approvals
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

-- Approvers hear that a request waits for them; the requester hears the outcome.
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, interrupts) VALUES
  (12, 'approval_requested', 'Approval Requested', 'When a request waits for your approval', 'ticket', '["in_app", "email"]', true),
  (13, 'approval_decided', 'Approval Decided', 'When your request is approved or declined', 'ticket', '["in_app", "email"]', false);
SELECT pg_catalog.setval('public.notification_types_id_seq', 13, true);

-- The outbox trigger also enqueues the two approval events. Same body as
-- 2026-09-27-000001 plus that predicate.
CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    IF NEW.event_type = 'comment.created'
       OR NEW.event_type = 'ticket_reference.added'
       OR NEW.event_type IN ('ticket.approval_requested', 'ticket.approval_decided')
       OR (NEW.event_type = 'ticket.created' AND NEW.data->>'requester_uuid' IS NOT NULL)
       OR (NEW.data ? 'previous_assignee_uuid'
           AND NEW.data->>'assignee_uuid' IS DISTINCT FROM NEW.data->>'previous_assignee_uuid')
       OR (NEW.data ? 'previous_workflow_state_id'
           AND NEW.data->>'workflow_state_id' IS DISTINCT FROM NEW.data->>'previous_workflow_state_id')
    THEN
        INSERT INTO notification_outbox (sync_id) VALUES (NEW.sync_id)
        ON CONFLICT (sync_id) DO NOTHING;
        PERFORM pg_notify('notification_outbox_new', '');
    END IF;
    RETURN NEW;
END;
$$;
