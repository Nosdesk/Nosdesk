-- Normalize ticket-merge metadata out of the wide `tickets` table.
--
-- A merge is a sparse, one-time event: `merged_into_ticket_id` / `merged_at` /
-- `merged_by_user_uuid` / `merge_reason` are NULL on ~99% of tickets and
-- describe a relationship ("this ticket was merged into that one"), not a core
-- attribute of a live ticket. Four cold columns on the hot tickets row is the
-- wrong shape, so they move to a 1:1 satellite keyed by the source ticket.
--
-- Not audited separately: the merge action already emits a `ticket.merged`
-- sync event and the source ticket's workflow-state change is audited on
-- `tickets`, so the satellite carries no new audit surface (and the backfill
-- below stays trigger-free).

CREATE TABLE public.ticket_merges (
    ticket_id integer NOT NULL,
    merged_into_ticket_id integer NOT NULL,
    merged_at timestamp with time zone NOT NULL,
    merged_by_user_uuid uuid,
    merge_reason text,
    workspace_id integer DEFAULT (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer NOT NULL,
    CONSTRAINT ticket_merges_pkey PRIMARY KEY (ticket_id)
);

-- Backfill BEFORE enabling RLS so the cross-workspace copy isn't filtered by
-- the per-row policy. (The migrator bypasses RLS anyway, but this is robust to
-- a non-superuser migrator.)
INSERT INTO public.ticket_merges
    (ticket_id, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, workspace_id)
SELECT id, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, workspace_id
FROM public.tickets
WHERE merged_into_ticket_id IS NOT NULL;

ALTER TABLE ONLY public.ticket_merges
    ADD CONSTRAINT ticket_merges_ticket_id_fkey
    FOREIGN KEY (ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.ticket_merges
    ADD CONSTRAINT ticket_merges_merged_into_ticket_id_fkey
    FOREIGN KEY (merged_into_ticket_id) REFERENCES public.tickets(id) ON DELETE CASCADE;
ALTER TABLE ONLY public.ticket_merges
    ADD CONSTRAINT ticket_merges_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id);

CREATE INDEX idx_ticket_merges_into ON public.ticket_merges USING btree (merged_into_ticket_id);

ALTER TABLE ONLY public.ticket_merges FORCE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_merges OWNER TO nosdesk_admin;
ALTER TABLE public.ticket_merges ENABLE ROW LEVEL SECURITY;

CREATE POLICY ticket_merges_workspace_isolation ON public.ticket_merges
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.ticket_merges TO nosdesk_app;

-- Drop the columns + their all-or-nothing check from the hot table.
ALTER TABLE public.tickets DROP CONSTRAINT tickets_merge_complete;
ALTER TABLE public.tickets
    DROP COLUMN merged_into_ticket_id,
    DROP COLUMN merged_at,
    DROP COLUMN merged_by_user_uuid,
    DROP COLUMN merge_reason;
