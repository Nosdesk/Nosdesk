-- Rules can run on a plugin's events (docs/plans/1.2-automation.md, A16).
--
-- A new enum value can't be used in the transaction that adds it, so this
-- migration only adds it; the next one queues plugin events for the rules
-- that use it.
ALTER TYPE public.rule_trigger_kind ADD VALUE IF NOT EXISTS 'plugin_event';
