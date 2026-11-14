-- Rules engine phase 2 schema (docs/plans/1.2-automation.md, A2).

-- 1. Paused rules move to the new state. Every `dry_run` row today is a rule
-- an admin paused, and `dry_run` now means a shadow run. This is a relabel,
-- not an edit: the version trigger stays off so no rule gains a version or a
-- new updated_at, and saved versions are relabelled too so their history
-- keeps reading as paused.
ALTER TABLE public.rules DISABLE TRIGGER rules_version_on_update;
UPDATE public.rules SET state = 'paused' WHERE state = 'dry_run';
ALTER TABLE public.rules ENABLE TRIGGER rules_version_on_update;
UPDATE public.rule_versions SET state = 'paused' WHERE state = 'dry_run';

-- A dry run shadows an automated trigger; a manual rule has none.
ALTER TABLE public.rules ADD CONSTRAINT rules_manual_no_dry_run
    CHECK (trigger_kind <> 'manual' OR state <> 'dry_run');

-- 2. The events rules react to. The sibling of notification_outbox: an AFTER
-- INSERT trigger on sync_actions enqueues the rows a rule trigger can act on,
-- and a dispatcher claims them (FOR UPDATE SKIP LOCKED). Delivery is at least
-- once; a failed attempt reschedules with backoff, and after the last attempt
-- the row is kept with dead_at set. workspace_id is copied from the sync
-- action so a claim can scope and cap by workspace without the join.
CREATE TABLE public.rule_event_outbox (
    sync_id         bigint      PRIMARY KEY,
    workspace_id    integer     NOT NULL
                    REFERENCES public.workspaces(id) ON DELETE CASCADE,
    enqueued_at     timestamptz NOT NULL DEFAULT now(),
    attempts        smallint    NOT NULL DEFAULT 0,
    claimed_at      timestamptz,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    -- An error kind, never a message body.
    last_error      text,
    dead_at         timestamptz
);
ALTER TABLE public.rule_event_outbox OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.rule_event_outbox TO nosdesk_app;

CREATE INDEX rule_event_outbox_due_idx
    ON public.rule_event_outbox (next_attempt_at)
    WHERE dead_at IS NULL;

ALTER TABLE public.rule_event_outbox ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.rule_event_outbox FORCE ROW LEVEL SECURITY;
CREATE POLICY rule_event_outbox_workspace_isolation ON public.rule_event_outbox
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

-- Only the events a rule trigger reads, named one by one so a new event type
-- is a decision rather than a match: ticket creation, the ticket field writes
-- (update_ticket_partial's event types and tag changes), and public comments
-- (ticket_replied). Rule bookkeeping (ticket.rule_applied), SLA stamps,
-- watchers, cycles, links, merges and plugin rows stay out.
--
-- Then only in a workspace with a rule an event can run: an automated trigger
-- (time-elapsed included, whose clocks the same events keep) that is live or
-- in dry run. Elsewhere nothing would ever claim the row. A rule that goes
-- live sees the events from then on.
--
-- ON CONFLICT DO NOTHING for the same reason webhook_outbox_enqueue has it: an
-- occurred_at change across a month boundary re-fires the trigger.
CREATE OR REPLACE FUNCTION public.rule_event_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path = public, pg_temp
    AS $$
BEGIN
    IF NEW.event_type IN (
           'ticket.created',
           'ticket.updated',
           'ticket.workflow_state_changed',
           'ticket.assignee_changed',
           'ticket.priority_changed',
           'ticket.title_changed',
           'ticket.category_changed',
           'ticket.verification_changed',
           'ticket.resolution_notes_changed',
           'ticket.tags_changed')
       OR (NEW.event_type = 'comment.created' AND NEW.data->>'is_internal' = 'false')
    THEN
        IF EXISTS (
            SELECT 1 FROM public.rules
            WHERE workspace_id = NEW.workspace_id
              AND archived_at IS NULL
              AND state IN ('live', 'dry_run')
              AND trigger_kind <> 'manual'
        ) THEN
            INSERT INTO public.rule_event_outbox (sync_id, workspace_id)
            VALUES (NEW.sync_id, NEW.workspace_id)
            ON CONFLICT (sync_id) DO NOTHING;
            PERFORM pg_notify('rule_event_outbox_new', '');
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.rule_event_outbox_enqueue() OWNER TO nosdesk_admin;

-- The trigger's lookup, one probe per qualifying event.
CREATE INDEX rules_event_fireable_idx ON public.rules (workspace_id)
    WHERE archived_at IS NULL
      AND state IN ('live', 'dry_run')
      AND trigger_kind <> 'manual';

CREATE TRIGGER tr_sync_actions_rule_event_outbox
    AFTER INSERT ON public.sync_actions
    FOR EACH ROW EXECUTE FUNCTION public.rule_event_outbox_enqueue();

-- 3. The recursion budget: each rule fires at most once per (root, ticket),
-- where the root is the event a cascade started from (decision D4). Nothing
-- wrote the table before, and a budget row is worthless after the hour the
-- sweeper keeps it, so it starts empty.
DELETE FROM public.ticket_rule_runs;
ALTER TABLE public.ticket_rule_runs RENAME COLUMN event_id TO root_id;
ALTER TABLE public.ticket_rule_runs
    ADD COLUMN workspace_id integer NOT NULL
        DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
        REFERENCES public.workspaces(id) ON DELETE CASCADE;

ALTER TABLE public.ticket_rule_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_rule_runs FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_rule_runs_workspace_isolation ON public.ticket_rule_runs
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
