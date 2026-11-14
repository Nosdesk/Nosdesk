DROP POLICY IF EXISTS ticket_rule_runs_workspace_isolation ON public.ticket_rule_runs;
ALTER TABLE public.ticket_rule_runs NO FORCE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_rule_runs DISABLE ROW LEVEL SECURITY;
ALTER TABLE public.ticket_rule_runs DROP COLUMN IF EXISTS workspace_id;
ALTER TABLE public.ticket_rule_runs RENAME COLUMN root_id TO event_id;

DROP TRIGGER IF EXISTS tr_sync_actions_rule_event_outbox ON public.sync_actions;
DROP FUNCTION IF EXISTS public.rule_event_outbox_enqueue();
DROP TABLE IF EXISTS public.rule_event_outbox;

-- Shadow runs have no phase 1 state; they go back to draft. Paused goes back
-- to dry_run, phase 1's name for it.
ALTER TABLE public.rules DROP CONSTRAINT IF EXISTS rules_manual_no_dry_run;
ALTER TABLE public.rules DISABLE TRIGGER rules_version_on_update;
UPDATE public.rules SET state = 'draft' WHERE state = 'dry_run';
UPDATE public.rules SET state = 'dry_run' WHERE state = 'paused';
ALTER TABLE public.rules ENABLE TRIGGER rules_version_on_update;
UPDATE public.rule_versions SET state = 'draft' WHERE state = 'dry_run';
UPDATE public.rule_versions SET state = 'dry_run' WHERE state = 'paused';
