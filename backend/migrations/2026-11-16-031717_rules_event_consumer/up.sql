-- Event rules run (docs/plans/1.2-automation.md, A6).

-- 1. What a rule skipped without leaving a rule_applications row: an event
-- turned away before any rule ran (its ticket held for approval, or merged),
-- and, for a live rule, an event whose ticket didn't meet its conditions. A
-- dry run records its condition skips as rows instead. The version trigger
-- lists the columns it watches, so a bump writes no rule version.
ALTER TABLE public.rules
    ADD COLUMN skipped_preflight_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN skipped_condition_unmet_count bigint NOT NULL DEFAULT 0;

-- 2. The event that took a budget slot, so a retried event can tell its own
-- earlier run of a rule (done, skipped quietly) from another event in the
-- same chain (suppressed). Nothing writes the table before this migration.
DELETE FROM public.ticket_rule_runs;
ALTER TABLE public.ticket_rule_runs ADD COLUMN sync_id bigint NOT NULL;

-- 3. An event queued again on purpose (a request released from approval runs
-- its ticket_created rules then). Its event may be days old, so the
-- dispatcher's check for backdated events leaves it alone.
ALTER TABLE public.rule_event_outbox
    ADD COLUMN rerun boolean NOT NULL DEFAULT false;
