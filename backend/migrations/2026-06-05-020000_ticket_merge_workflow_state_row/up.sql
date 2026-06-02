-- Seeds the built-in `Merged` workflow state, one row per workspace
-- (workflow_states is workspace-scoped: workspace_id is NOT NULL and the
-- default / category-position unique indexes are per workspace). Source
-- tickets in a merge transition here. `pauses_sla` defaults TRUE, so the
-- SLA engine freezes a merged ticket's clock the same way it does for
-- `done` with no policy change needed. `is_default` stays FALSE: the
-- per-workspace partial unique index allows only one default state per
-- workspace (currently Backlog), and the merge handler selects this row
-- by category, not by the default flag. The `subtle` colour reuses the
-- neutral badge palette, visually distinct from `done` (green).
--
-- Suppress the audit trigger for this schema-time seed: workflow_states
-- is audited, and a migration has no actor or workspace to attribute the
-- rows to (the same idiom as the pauses_sla backfill).
SET LOCAL session_replication_role = 'replica';

INSERT INTO workflow_states (name, category, color, position, is_default, workspace_id)
SELECT 'Merged', 'merged', 'subtle', 0, FALSE, w.id
FROM workspaces w;

SET LOCAL session_replication_role = 'origin';
