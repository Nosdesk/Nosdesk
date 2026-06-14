-- workspace_members was the only tenant table shipped without row-level
-- security: every other tenant table has ENABLE + FORCE RLS and a
-- <t>_workspace_isolation policy keyed on app.workspace_id, but
-- workspace_members relied solely on per-query WHERE clauses for
-- isolation. Bring it in line with the canonical pattern so a query that
-- forgets the workspace predicate can't leak/clobber across workspaces.
--
-- Safe because every write path is either on the BYPASSRLS role
-- (nosdesk_admin, via with_actor_bypass_context / PlatformConn) or runs
-- as nosdesk_app with app.workspace_id pinned to the row's workspace
-- (with_actor_context). The one exception, the self-register rollback
-- purge in handlers/auth.rs, was routed through bypass in the same change.
ALTER TABLE public.workspace_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_members FORCE ROW LEVEL SECURITY;

CREATE POLICY workspace_members_workspace_isolation ON public.workspace_members
  USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
  WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
