-- Revert the predicate to the single-clause form.
UPDATE saved_views
SET filter = jsonb_set(
    filter,
    '{predicate,children}',
    '[{ "field": "triage_state", "op": "eq", "value": "untriaged" }]'::jsonb
)
WHERE scope = 'workspace'
  AND name = 'Triage'
  AND archived_at IS NULL
  AND jsonb_array_length(filter -> 'predicate' -> 'children') = 2;
