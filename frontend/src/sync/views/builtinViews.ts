/**
 * Built-in saved views shipped with the platform.
 *
 * Phase 5 hard-codes Triage and My Queue here so the views are
 * available without a saved-views CRUD surface (which lands in a
 * follow-up). The shape mirrors what a SavedView row would carry
 * — that's deliberate so the migration to DB-backed saved views
 * can replace `BUILTIN_VIEWS` with a fetch + cache without
 * touching consumers.
 */
import type { FilterState, ListViewShape } from './types'

export interface BuiltInView {
  /** Stable id used in the URL (`?view=triage`). */
  id: 'triage' | 'my-queue'
  name: string
  description: string
  shape: ListViewShape
  filter: FilterState
}

const baseListShape: Omit<ListViewShape, 'columns'> = {
  type: 'list',
  group_by: { primary: 'workflow_state.category' },
  sort: [{ field: 'last_activity_at', dir: 'desc' }],
  visible_card_fields: [
    'title',
    'workflow_state',
    'priority',
    'assignee_uuid',
    'last_activity_at',
  ],
  card_density: 'comfortable',
  swimlane_collapse_state: {},
  filter_id: null,
  row_height: 'compact',
}

const defaultColumns: ListViewShape['columns'] = [
  { field: 'id', width: 80, pinned: 'left' },
  { field: 'title', sortable: true },
  { field: 'workflow_state', width: 140, sortable: true },
  { field: 'priority', width: 100, sortable: true },
  { field: 'assignee_uuid', width: 160 },
  { field: 'last_activity_at', width: 160, sortable: true },
]

const baseFilter: Omit<FilterState, 'predicate' | 'quick_filters'> = {
  scope: {
    project_ids: 'all',
    cycle_ids: 'all',
    group_ids: 'all',
    include_archived: false,
  },
}

export const TRIAGE_VIEW: BuiltInView = {
  id: 'triage',
  name: 'Triage',
  description: 'Tickets in the triage category, awaiting initial sort',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    // Architecture spec calls for `triage_state = 'untriaged' AND cycle = null`
    // but neither field is in the current schema. Stand-in: filter
    // on workflow_state.category. Semantically equivalent today
    // and the predicate switches over to the spec'd fields when
    // they ship in Phase 6.
    predicate: {
      combinator: 'AND',
      children: [
        { field: 'workflow_state.category', op: 'eq', value: 'triage' },
      ],
    },
    quick_filters: [],
  },
}

export const MY_QUEUE_VIEW: BuiltInView = {
  id: 'my-queue',
  name: 'My Queue',
  description: 'Tickets assigned to you',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    predicate: { combinator: 'AND', children: [] },
    // `mine` quick filter pulls the current user's UUID from the
    // evaluator context, so this view round-trips through URL
    // serialization without baking a specific UUID in.
    quick_filters: ['mine'],
  },
}

export const BUILTIN_VIEWS: BuiltInView[] = [TRIAGE_VIEW, MY_QUEUE_VIEW]

export function findBuiltinView(id: string): BuiltInView | null {
  return BUILTIN_VIEWS.find((v) => v.id === id) ?? null
}
