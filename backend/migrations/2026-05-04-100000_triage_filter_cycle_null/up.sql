-- Tighten the seeded Triage filter to match the architecture spec
-- (§ 10 Phase 6): "triage_state = 'untriaged' AND cycle = NULL".
-- The original seed only had the triage_state clause because
-- cycle_id wasn't on the ticket sync payload yet; that landed
-- alongside this migration so the predicate now evaluates fully
-- client-side.
--
-- Idempotent: only updates the row if it still has the original
-- single-clause predicate, so a workspace that has customised
-- Triage by hand keeps its edits.

UPDATE saved_views
SET filter = jsonb_set(
    filter,
    '{predicate,children}',
    '[
      { "field": "triage_state", "op": "eq",       "value": "untriaged" },
      { "field": "cycle_id",     "op": "is_empty",  "value": null }
    ]'::jsonb
)
WHERE scope = 'workspace'
  AND name = 'Triage'
  AND archived_at IS NULL
  AND jsonb_array_length(filter -> 'predicate' -> 'children') = 1;
