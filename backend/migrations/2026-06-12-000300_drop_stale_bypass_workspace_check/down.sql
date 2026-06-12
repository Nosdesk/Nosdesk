-- Restore the original policies, with the (dead) bypass branch.

DROP POLICY bug_reports_workspace_isolation ON public.bug_reports;
CREATE POLICY bug_reports_workspace_isolation ON public.bug_reports
    USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)))
    WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));

DROP POLICY canned_response_insertions_workspace_isolation ON public.canned_response_insertions;
CREATE POLICY canned_response_insertions_workspace_isolation ON public.canned_response_insertions
    USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)))
    WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));

DROP POLICY rule_applications_workspace_isolation ON public.rule_applications;
CREATE POLICY rule_applications_workspace_isolation ON public.rule_applications
    USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)))
    WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));

DROP POLICY rule_versions_workspace_isolation ON public.rule_versions;
CREATE POLICY rule_versions_workspace_isolation ON public.rule_versions
    USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)))
    WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));

DROP POLICY rules_workspace_isolation ON public.rules;
CREATE POLICY rules_workspace_isolation ON public.rules
    USING (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)))
    WITH CHECK (((workspace_id = (NULLIF(current_setting('app.workspace_id'::text, true), ''::text))::integer) OR (NULLIF(current_setting('app.bypass_workspace_check'::text, true), ''::text) = 'true'::text)));
