-- Promote Triage from a hard-coded built-in to a workspace-default
-- saved view. Architecture doc § 10 Phase 6 names Triage as the
-- workspace-default; the built-in TRIAGE_VIEW in
-- frontend/src/sync/views/builtinViews.ts stays as a fallback for
-- bookmark URLs (`?view=triage`) but the in-app default lookup
-- now points at this row.
--
-- Filter: triage_state = 'untriaged'. The architecture spec also
-- calls for `cycle = NULL` in the predicate, but cycle membership
-- isn't denormalised onto CardData yet; that AND clause lands when
-- the sync engine emits cycle_id with the ticket payload.
--
-- created_by points at the first admin we can find. If none exists
-- (fresh install) the row inserts with created_by NULL — the
-- frontend tolerates that.

INSERT INTO saved_views (scope, scope_id, name, shape, filter, created_by, is_default)
SELECT
    'workspace',
    NULL,
    'Triage',
    '{
      "type": "list",
      "group_by": { "primary": "workflow_state.category" },
      "sort": [{ "field": "last_activity_at", "dir": "desc" }],
      "visible_card_fields": ["title","workflow_state","priority","assignee_uuid","last_activity_at"],
      "card_density": "comfortable",
      "swimlane_collapse_state": {},
      "filter_id": null,
      "row_height": "compact",
      "columns": [
        { "field": "id", "width": 80, "pinned": "left" },
        { "field": "title", "sortable": true },
        { "field": "workflow_state", "width": 140, "sortable": true },
        { "field": "priority", "width": 100, "sortable": true },
        { "field": "assignee_uuid", "width": 160 },
        { "field": "last_activity_at", "width": 160, "sortable": true }
      ]
    }'::jsonb,
    '{
      "predicate": {
        "combinator": "AND",
        "children": [
          { "field": "triage_state", "op": "eq", "value": "untriaged" }
        ]
      },
      "quick_filters": [],
      "scope": {
        "project_ids": "all",
        "cycle_ids": "all",
        "group_ids": "all",
        "include_archived": false
      }
    }'::jsonb,
    (SELECT uuid FROM users WHERE role = 'admin' ORDER BY created_at LIMIT 1),
    true
WHERE NOT EXISTS (
    SELECT 1 FROM saved_views
    WHERE scope = 'workspace' AND name = 'Triage' AND archived_at IS NULL
);
