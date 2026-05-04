/**
 * View-layer type contracts (CardData, ViewShape, FilterState).
 *
 * These are what the view components speak. The view never imports
 * a Pinia store, never imports the sync engine directly — it gets
 * `cards`, `dispatch`, and `presence` props and renders. The
 * dispatcher (in the parent route) translates view actions into
 * sync-engine writes.
 *
 * Phase 4 ships the kanban shape end-to-end; the other ViewShape
 * variants (list / calendar / gantt / scrum) are spec'd here so
 * the discriminated union compiles, but only `kanban` has a
 * renderer in this commit.
 */

import type { WorkflowStateCategory } from '@/types/workflow'

// ===================== CardData =====================
//
// One card row. Denormalised at the boundary, not in the renderer
// — the server pays the cost once at write time so the client can
// render thousands of cards without joins. Foreign keys are carried
// as ids; useReference() in the consumer resolves them.

export type Priority = 'low' | 'medium' | 'high' | 'urgent' | 'none'

export interface CardWorkflowState {
  id: number
  name: string
  category: WorkflowStateCategory
  color: string
}

export interface CardData {
  // Identity
  id: number
  uuid?: string

  // Core
  title: string
  workflow_state: CardWorkflowState
  priority: Priority

  // People (refs; resolve via useReference())
  assignee_uuid?: string | null
  requester_uuid?: string | null

  // Time
  due_date?: string | null
  created_at: string
  updated_at: string
  /** Sort key the kanban / list views default to. Driven by any
   * activity touching the row (status change, comment, etc.). */
  last_activity_at: string
  unread_activity?: boolean

  // Estimation (optional; reserved for cycles in Phase 6+).
  estimate?: { value: number; unit: 'points' | 'hours' | 'tshirt' } | null

  // Hierarchy
  parent_ticket_id?: number | null
  sub_issue_count?: { total: number; closed: number } | null

  // Relation counts (no embedded objects).
  relation_counts?: {
    blocks: number
    blocked_by: number
    related: number
    duplicate_of: number
  }

  // Cycle & triage (Phase 6).
  cycle_id?: number | null
  triage_state?: 'untriaged' | 'triaged' | 'rejected' | null

  // Visibility & categorisation.
  internal_only?: boolean
  category_id?: number | null

  // Pills (precomputed at the boundary, never derived in renderer).
  kb_gap_signal?: 'none' | 'weak' | 'strong'
  affected_devices?: {
    count: number
    first?: { id: number; name: string; os?: string | null }
  } | null
}

// ===================== ViewShape =====================

export type ViewShape =
  | KanbanViewShape
  | ListViewShape
  | CalendarViewShape
  | GanttViewShape
  | ScrumViewShape

export type FieldRef = string

export interface ViewShapeBase {
  /** Two-axis swimlane grouping. Primary becomes columns; optional
   * secondary becomes lanes inside each column. Phase 4 ships
   * primary only; secondary is reserved. */
  group_by: { primary: FieldRef; secondary?: FieldRef }
  sort: { field: FieldRef; dir: 'asc' | 'desc' }[]
  visible_card_fields: (keyof CardData)[]
  card_density: 'compact' | 'comfortable' | 'spacious'
  /** Per-swimlane collapse state, persisted with the saved view. */
  swimlane_collapse_state: Record<string, boolean>
  filter_id: string | null
  /** Reserved for Phase 5 presence wiring. */
  realtime_presence_scope?: 'view' | 'card' | 'off'
}

export interface KanbanViewShape extends ViewShapeBase {
  type: 'kanban'
  card_layout: 'standard' | 'compact'
  drag_handles: {
    allow_status_change: boolean
    allow_assignee_change: boolean
    allow_cycle_change: boolean
    allow_priority_change: boolean
  }
}

export interface ListViewShape extends ViewShapeBase {
  type: 'list'
  columns: {
    field: keyof CardData
    width?: number
    pinned?: 'left' | 'right'
    sortable?: boolean
  }[]
  row_height: 'compact' | 'comfortable'
}

export interface CalendarViewShape extends ViewShapeBase {
  type: 'calendar'
  time_axis: { unit: 'day' | 'week' | 'month'; start: string; end: string }
  date_field: 'due_date' | 'created_at' | 'last_activity_at'
}

export interface GanttViewShape extends ViewShapeBase {
  type: 'gantt'
  time_axis: { unit: 'day' | 'week' | 'month'; start: string; end: string }
  zoom_level: 'day' | 'week' | 'month' | 'quarter'
  show_dependencies: boolean
}

export interface ScrumViewShape extends ViewShapeBase {
  type: 'scrum'
  cycle_id: number
  show_burndown: boolean
  show_velocity: boolean
}

// ===================== FilterState =====================
//
// Structured form is what the picker UI builds. Query-language UI
// is parking lot. Phase 4 ships a thin slice — predicate +
// quick_filters + scope.project_ids — enough to feed the kanban a
// constrained card set.

export interface FilterPredicate {
  field: string
  op: FilterOp
  value: unknown
}

export type FilterOp =
  | 'eq' | 'neq'
  | 'in' | 'not_in'
  | 'has' | 'no'
  | 'gt' | 'lt' | 'gte' | 'lte' | 'between'
  | 'is_empty' | 'is_not_empty'
  | 'changed_in_last'

export interface FilterGroup {
  combinator: 'AND' | 'OR' | 'NOT'
  children: (FilterPredicate | FilterGroup)[]
}

export type QuickFilter =
  | 'mine' | 'unassigned' | 'overdue' | 'sla_at_risk'
  | 'mentions_me' | 'starred' | 'has_kb_gap'
  | 'recently_updated' | 'in_my_cycles'

export interface FilterState {
  predicate: FilterGroup
  quick_filters: QuickFilter[]
  scope: {
    project_ids: number[] | 'all'
    cycle_ids: number[] | 'all'
    group_ids: number[] | 'all'
    include_archived: boolean
  }
  text_query?: string
}

// ===================== Render contract =====================

import type { ComputedRef } from 'vue'

export interface ViewRenderProps<S extends ViewShape> {
  shape: S
  cards: ComputedRef<readonly CardData[]>
  groupedCards: ComputedRef<Map<string, readonly CardData[]>>
  filter: FilterState
  isLoading: boolean
  dispatch: (action: ViewAction) => void
}

export type ViewAction =
  | { kind: 'card-move'; cardId: number; targetLane: string; targetIndex?: number }
  | { kind: 'card-update'; cardId: number; patch: Partial<CardData> }
  | { kind: 'card-create'; defaults: Partial<CardData> }
  | { kind: 'card-archive'; cardId: number }
  | { kind: 'filter-change'; filter: FilterState }
  | { kind: 'sort-change'; sort: ViewShape['sort'] }
  | { kind: 'group-change'; group_by: ViewShape['group_by'] }
  | { kind: 'bulk-update'; cardIds: number[]; patch: Partial<CardData> }

// ===================== Defaults =====================

/** Sensible default kanban shape — single-axis swimlanes by
 * workflow state category. Used as the seed for projects that
 * haven't saved a view yet. */
export function defaultKanbanShape(): KanbanViewShape {
  return {
    type: 'kanban',
    card_layout: 'standard',
    group_by: { primary: 'workflow_state.category' },
    sort: [{ field: 'last_activity_at', dir: 'desc' }],
    visible_card_fields: ['title', 'priority', 'assignee_uuid', 'due_date'],
    card_density: 'comfortable',
    swimlane_collapse_state: {},
    filter_id: null,
    drag_handles: {
      allow_status_change: true,
      allow_assignee_change: false,
      allow_cycle_change: false,
      allow_priority_change: false,
    },
  }
}

export function defaultFilterState(projectId: number | 'all'): FilterState {
  return {
    predicate: { combinator: 'AND', children: [] },
    quick_filters: [],
    scope: {
      project_ids: projectId === 'all' ? 'all' : [projectId],
      cycle_ids: 'all',
      group_ids: 'all',
      include_archived: false,
    },
  }
}
