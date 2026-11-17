-- Time-elapsed rules' clocks (docs/plans/1.2-automation.md, A9).
--
-- One row per running clock: a time-elapsed rule waiting on a ticket. The
-- rules engine keeps the rows (a reply, a state change or an assignment
-- restarts or stops a clock; a reconcile sweep repairs drift), and a worker
-- claims the due ones (FOR UPDATE SKIP LOCKED, with a lease on claimed_at),
-- re-reads the clock and fires the rule. Clocks are derived in Rust, so no
-- trigger writes this table. clock_name is free text: the engine owns the
-- list of clocks.
CREATE TABLE public.ticket_elapsed_queue (
    workspace_id     integer     NOT NULL
                     DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                     REFERENCES public.workspaces(id) ON DELETE CASCADE,
    ticket_id        integer     NOT NULL,
    rule_id          integer     NOT NULL,
    clock_name       text        NOT NULL,
    -- When the clock started: the reply, change or creation it counts from.
    clock_started_at timestamptz NOT NULL,
    -- When the rule fires, or retries after a failed attempt.
    due_at           timestamptz NOT NULL,
    claimed_at       timestamptz,
    attempts         smallint    NOT NULL DEFAULT 0,
    PRIMARY KEY (ticket_id, rule_id, clock_name),
    CONSTRAINT ticket_elapsed_queue_ticket_id_fkey FOREIGN KEY (workspace_id, ticket_id)
        REFERENCES public.tickets (workspace_id, id) ON DELETE CASCADE,
    CONSTRAINT ticket_elapsed_queue_rule_id_fkey FOREIGN KEY (workspace_id, rule_id)
        REFERENCES public.rules (workspace_id, id) ON DELETE CASCADE
);
ALTER TABLE public.ticket_elapsed_queue OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.ticket_elapsed_queue TO nosdesk_app;

-- The claim scans due rows, stale claims included, so the index is not
-- partial on claimed_at.
CREATE INDEX ticket_elapsed_queue_due_idx ON public.ticket_elapsed_queue (due_at);
-- A rule's rows: the cascade when it is deleted, and the requeue when it is
-- edited or goes live.
CREATE INDEX ticket_elapsed_queue_rule_idx ON public.ticket_elapsed_queue (rule_id);

ALTER TABLE public.ticket_elapsed_queue ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_elapsed_queue FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_elapsed_queue_workspace_isolation ON public.ticket_elapsed_queue
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
