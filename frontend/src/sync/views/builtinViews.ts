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
import type { CalendarViewShape, FilterState, ListViewShape } from './types'

export interface BuiltInView {
  /** Stable id used in the URL (`?view=my-open`). */
  id: 'my-open' | 'all-active' | 'triage' | 'calendar'
  name: string
  description: string
  /** Built-ins ship list and calendar shapes today. The TicketsList
   * view branches on `shape.type` to pick the renderer. */
  shape: ListViewShape | CalendarViewShape
  filter: FilterState
}

const baseListShape: Omit<ListViewShape, 'columns'> = {
  type: 'list',
  group_by: { primary: 'workflow_state.category' },
  sort: [{ field: 'last_activity_at', dir: 'desc' }],
  // Ticket id leads — it's the row's reference handle (people
  // say "looking at #1234" in chat / email / commits) and a
  // short, scannable anchor that aligns flush left for quick
  // scanning. Title sits second as the primary content. Bug
  // history: leaving `id` out of this list meant `Reset
  // columns` returned to a no-id layout, and re-toggling id
  // through the picker appended it to the rightmost slot —
  // contradicting the registry's `defaultVisible: true` for id.
  visible_card_fields: [
    'id',
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

/** Default landing view for an active agent. Shows tickets the
 * current user owns. Resolution chain falls back to ALL_ACTIVE_VIEW
 * inline when this is empty (TicketsListView handles the swap), so
 * an agent with no assignments never lands on a blank screen.
 *
 * Naming: peer helpdesks (Zendesk "Your unsolved", Help Scout
 * "Mine", Front "Assigned to me") all use "Mine"-style language.
 * "My Open" is the most explicit since it scopes to non-resolved
 * by virtue of the ALL_ACTIVE fall-through. */
export const MY_OPEN_VIEW: BuiltInView = {
  id: 'my-open',
  name: 'My Open',
  description: 'Open tickets assigned to you',
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

/** Workspace-wide active queue: every ticket that isn't done or
 * cancelled. Industry equivalent: GitHub Issues' default
 * `is:open` filter, JSM's "All open queue", HubSpot's pipeline
 * board. Used as the smart-fall-through target when MY_OPEN is
 * empty so the default landing surface is never blank for an
 * established workspace. */
export const ALL_ACTIVE_VIEW: BuiltInView = {
  id: 'all-active',
  name: 'All Active',
  description: 'Every ticket that hasn\'t been resolved or cancelled',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    predicate: {
      combinator: 'AND',
      children: [
        { field: 'workflow_state.category', op: 'not_in', value: ['done', 'cancelled'] },
      ],
    },
    quick_filters: [],
  },
}

export const TRIAGE_VIEW: BuiltInView = {
  id: 'triage',
  name: 'Triage',
  description: 'Tickets awaiting initial categorisation',
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

/** @deprecated use MY_OPEN_VIEW. Kept as a transitional alias so
 * any in-flight URL with `?view=my-queue` still resolves; remove
 * once telemetry shows zero hits over a 30-day window. */
export const MY_QUEUE_VIEW = MY_OPEN_VIEW

/** Calendar view of every ticket with a due_date. Anchors on
 * `due_date` because that's what calendars are for; created_at /
 * last_activity_at are reserved as alternate anchors when the
 * shape gets a saved-view editor. */
const baseCalendarShape: CalendarViewShape = {
  type: 'calendar',
  group_by: { primary: 'workflow_state.category' },
  sort: [{ field: 'due_date', dir: 'asc' }],
  visible_card_fields: ['title', 'priority', 'assignee_uuid', 'due_date'],
  card_density: 'compact',
  swimlane_collapse_state: {},
  filter_id: null,
  time_axis: { unit: 'month', start: '', end: '' },
  date_field: 'due_date',
}

export const CALENDAR_VIEW: BuiltInView = {
  id: 'calendar',
  name: 'Calendar',
  description: 'Tickets placed on the day they are due',
  shape: baseCalendarShape,
  filter: {
    ...baseFilter,
    predicate: { combinator: 'AND', children: [] },
    quick_filters: [],
  },
}

/** Order matters: the first item is the visual default in the
 * sidebar / view-switcher, and the resolution chain falls back to
 * MY_OPEN_VIEW (also the first entry) when nothing else matches.
 * Re-ordering changes the perceived front door of the product. */
export const BUILTIN_VIEWS: BuiltInView[] = [
  MY_OPEN_VIEW,
  ALL_ACTIVE_VIEW,
  TRIAGE_VIEW,
  CALENDAR_VIEW,
]

export function findBuiltinView(id: string): BuiltInView | null {
  // Accept the legacy `my-queue` id so bookmarks / shared URLs
  // from before the rename keep working. Resolves to MY_OPEN_VIEW.
  if (id === 'my-queue') return MY_OPEN_VIEW
  return BUILTIN_VIEWS.find((v) => v.id === id) ?? null
}
