ALTER TABLE public.rule_event_outbox DROP COLUMN IF EXISTS rerun;
ALTER TABLE public.ticket_rule_runs DROP COLUMN IF EXISTS sync_id;
ALTER TABLE public.rules
    DROP COLUMN IF EXISTS skipped_condition_unmet_count,
    DROP COLUMN IF EXISTS skipped_preflight_count;
