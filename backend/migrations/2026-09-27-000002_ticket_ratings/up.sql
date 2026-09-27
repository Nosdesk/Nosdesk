-- Requester satisfaction (docs/plans/v1.1-requester-experience.md, P2.2).
--
-- One row per ticket and rater: the requester's latest answer to "is it
-- fixed?". Answering again updates it. `comment` is the optional note left
-- with a "fixed" answer; a "still need help" answer travels as a reply.
CREATE TABLE public.ticket_ratings (
    ticket_id    integer     NOT NULL REFERENCES public.tickets(id) ON DELETE CASCADE,
    rater_uuid   uuid        NOT NULL REFERENCES public.users(uuid) ON DELETE CASCADE,
    rating       text        NOT NULL CHECK (rating IN ('good', 'bad')),
    comment      text,
    workspace_id integer     NOT NULL
                 DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                 REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ticket_id, rater_uuid)
);
ALTER TABLE public.ticket_ratings OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.ticket_ratings TO nosdesk_app;

CREATE INDEX ticket_ratings_workspace_idx ON public.ticket_ratings (workspace_id, updated_at);

ALTER TABLE public.ticket_ratings ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_ratings FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_ratings_workspace_isolation ON public.ticket_ratings
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
