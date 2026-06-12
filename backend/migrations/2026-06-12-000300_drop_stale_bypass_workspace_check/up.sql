-- Remove the dead `app.bypass_workspace_check` escape hatch from the
-- five RLS policies that still carry it (P1.5).
--
-- Phase 3h.4 moved workspace-isolation bypass from a GUC flag to a
-- dedicated BYPASSRLS role (nosdesk_admin), and nothing has set this GUC
-- since. But these five policies kept the `OR current_setting(...) =
-- 'true'` branch, leaving a latent isolation-disable switch: anything
-- that set the GUC would silently drop tenant isolation on these tables.
-- The other 81 tenant tables already use the bare workspace_id form;
-- recreate these to match it exactly (PERMISSIVE / FOR ALL / TO public
-- defaults, USING == WITH CHECK).

DROP POLICY bug_reports_workspace_isolation ON public.bug_reports;
CREATE POLICY bug_reports_workspace_isolation ON public.bug_reports
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

DROP POLICY canned_response_insertions_workspace_isolation ON public.canned_response_insertions;
CREATE POLICY canned_response_insertions_workspace_isolation ON public.canned_response_insertions
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

DROP POLICY rule_applications_workspace_isolation ON public.rule_applications;
CREATE POLICY rule_applications_workspace_isolation ON public.rule_applications
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

DROP POLICY rule_versions_workspace_isolation ON public.rule_versions;
CREATE POLICY rule_versions_workspace_isolation ON public.rule_versions
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));

DROP POLICY rules_workspace_isolation ON public.rules;
CREATE POLICY rules_workspace_isolation ON public.rules
    USING ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer))
    WITH CHECK ((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer));
