-- Ticket references in comments (docs/plans/ticket-mentions-in-comments.md).
--
-- A `ticket_link` node in a comment is a reference from that comment to
-- another ticket. The comment write records each one here and emits one
-- `ticket_reference.added` sync action per reference, which the referenced
-- ticket's activity feed and the notification deriver consume.
--
-- No `ticket_id` copy: a merge moves comments between tickets, and the
-- source is always `comments.ticket_id`. The enum value is only added here,
-- never used until a later runtime emit (as with `asset_loan`).
ALTER TYPE public.sync_aggregate ADD VALUE IF NOT EXISTS 'ticket_reference';

CREATE TABLE public.comment_ticket_references (
    comment_id           integer     NOT NULL REFERENCES public.comments(id) ON DELETE CASCADE,
    referenced_ticket_id integer     NOT NULL REFERENCES public.tickets(id) ON DELETE CASCADE,
    workspace_id         integer     NOT NULL
                         DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                         REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (comment_id, referenced_ticket_id)
);
ALTER TABLE public.comment_ticket_references OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.comment_ticket_references TO nosdesk_app;

-- "Which comments mention this ticket."
CREATE INDEX comment_ticket_references_referenced_idx
    ON public.comment_ticket_references (workspace_id, referenced_ticket_id);

ALTER TABLE public.comment_ticket_references ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.comment_ticket_references FORCE ROW LEVEL SECURITY;
CREATE POLICY comment_ticket_references_workspace_isolation ON public.comment_ticket_references
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

INSERT INTO public.notification_types (id, code, name, description, category, default_channels) VALUES
  (11, 'ticket_referenced', 'Ticket Mentioned', 'When a ticket assigned to you is mentioned in a comment', 'mention', '["in_app", "email"]');
SELECT pg_catalog.setval('public.notification_types_id_seq', 11, true);

-- The outbox trigger enqueues the new event alongside the rows it already
-- acts on. Same body as 2026-09-14-000001 plus the one predicate.
CREATE OR REPLACE FUNCTION public.notification_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    IF NEW.event_type = 'comment.created'
       OR NEW.event_type = 'ticket_reference.added'
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
