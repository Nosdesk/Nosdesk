-- Rules engine phase 2 (docs/plans/1.2-automation.md, decision D1).
--
-- Phase 1 used `dry_run` to mean Paused. Phase 2 gives `dry_run` the plan's
-- meaning (shadow runs of an automated rule) and Paused its own value. A new
-- enum value can't be used in the transaction that adds it, so this migration
-- only adds it; the next one moves the paused rules over.
ALTER TYPE public.rule_state ADD VALUE IF NOT EXISTS 'paused' AFTER 'live';
