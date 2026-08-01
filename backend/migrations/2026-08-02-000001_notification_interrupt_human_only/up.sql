-- Per-user "only interrupt me for human-originated events" toggle.
-- When true, notifications triggered by a non-human actor (system jobs
-- like SLA breach / loan overdue, or rule-based auto-assignment) land in
-- the bell without a toast / desktop popup, even when their type is
-- otherwise interrupting. Human-originated notifications are unaffected.
-- NOT NULL DEFAULT false is a metadata-only add (no row rewrite, no
-- per-row UPDATE), so it does not fire the audit trigger.
ALTER TABLE user_preferences
    ADD COLUMN interrupt_human_only BOOLEAN NOT NULL DEFAULT false;
