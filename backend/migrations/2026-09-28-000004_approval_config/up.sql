-- Approvals, configuration (docs/plans/requester-phase3.md, "Approvals: agreed
-- design"). Invisible until a request type turns it on.

-- Workspace behaviour: how waiting tickets show, who may skip an approval, and
-- whether an unanswered one approves itself after some days.
ALTER TABLE public.site_settings
    ADD COLUMN approval_waiting_display text NOT NULL DEFAULT 'badge'
        CHECK (approval_waiting_display IN ('badge', 'held')),
    ADD COLUMN approval_skip_by text NOT NULL DEFAULT 'admins'
        CHECK (approval_skip_by IN ('nobody', 'admins', 'agents')),
    ADD COLUMN approval_auto_approve_days integer
        CHECK (approval_auto_approve_days BETWEEN 1 AND 90);

-- Per request type: whether it needs approval, from whom, and whether one
-- approval is enough or everyone's is needed.
ALTER TABLE public.ticket_categories
    ADD COLUMN approval_required boolean NOT NULL DEFAULT false,
    ADD COLUMN approval_rule text NOT NULL DEFAULT 'any'
        CHECK (approval_rule IN ('any', 'all')),
    ADD COLUMN approval_by_manager boolean NOT NULL DEFAULT false;

-- Named approvers for a request type.
CREATE TABLE public.category_approvers (
    category_id  integer     NOT NULL REFERENCES public.ticket_categories(id) ON DELETE CASCADE,
    user_uuid    uuid        NOT NULL REFERENCES public.users(uuid) ON DELETE CASCADE,
    workspace_id integer     NOT NULL
                 DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                 REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (category_id, user_uuid)
);
ALTER TABLE public.category_approvers OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.category_approvers TO nosdesk_app;
CREATE INDEX category_approvers_workspace_idx ON public.category_approvers (workspace_id);
ALTER TABLE public.category_approvers ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.category_approvers FORCE ROW LEVEL SECURITY;
CREATE POLICY category_approvers_workspace_isolation ON public.category_approvers
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);
CREATE TRIGGER tr_audit_category_approvers AFTER INSERT OR DELETE OR UPDATE ON public.category_approvers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('category_id');

-- A person's manager in this workspace (the "requester's manager" approver).
ALTER TABLE public.user_profiles
    ADD COLUMN manager_uuid uuid REFERENCES public.users(uuid) ON DELETE SET NULL;
