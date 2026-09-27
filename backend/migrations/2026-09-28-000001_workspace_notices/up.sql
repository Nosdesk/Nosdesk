-- Known-issue notices (docs/plans/requester-phase3.md, item 1): during an
-- outage the team posts "we know, we're on it" on the portal and the guest
-- form so people don't file duplicates. Every notice has an end time (a stale
-- notice is worse than none). An optional incident ticket lets signed-in
-- requesters follow the issue instead of opening their own ticket.
CREATE TABLE public.workspace_notices (
    id                 serial      PRIMARY KEY,
    title              varchar(120) NOT NULL,
    body               text,
    severity           text        NOT NULL DEFAULT 'info'
                       CHECK (severity IN ('info', 'degraded', 'outage')),
    starts_at          timestamptz NOT NULL DEFAULT now(),
    ends_at            timestamptz NOT NULL,
    incident_ticket_id integer     REFERENCES public.tickets(id) ON DELETE SET NULL,
    created_by         uuid        REFERENCES public.users(uuid) ON DELETE SET NULL,
    workspace_id       integer     NOT NULL
                       DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                       REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CHECK (ends_at > starts_at)
);
ALTER TABLE public.workspace_notices OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_notices TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.workspace_notices_id_seq TO nosdesk_app;

CREATE INDEX workspace_notices_active_idx ON public.workspace_notices (workspace_id, ends_at);

ALTER TABLE public.workspace_notices ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_notices FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_notices_workspace_isolation ON public.workspace_notices
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

CREATE TRIGGER tr_audit_workspace_notices AFTER INSERT OR DELETE OR UPDATE ON public.workspace_notices
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');
