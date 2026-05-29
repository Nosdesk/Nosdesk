-- Per-workflow-state SLA pause flag. Previously the SLA matcher hard-
-- coded "anything not category=active pauses the clock"; this lifts
-- that into per-state control so admins can keep a status like
-- "Waiting on customer" paused even if it's modelled under the active
-- category, or run the clock through a custom triage flow.
ALTER TABLE workflow_states
    ADD COLUMN pauses_sla BOOLEAN NOT NULL DEFAULT TRUE;

-- Preserve the existing engine semantics for existing rows: before
-- this migration, only category=active ran the clock. Suppress the
-- audit trigger for the duration of the backfill, the same idiom used
-- elsewhere in the migration set: a schema-time backfill has no
-- meaningful actor or workspace to attribute the rows to, and the
-- audit_log NOT NULL workspace_id constraint would otherwise reject
-- the write.
SET LOCAL session_replication_role = 'replica';
UPDATE workflow_states SET pauses_sla = FALSE WHERE category = 'active';
SET LOCAL session_replication_role = 'origin';
