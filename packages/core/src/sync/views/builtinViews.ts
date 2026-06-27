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
  id:
    | 'my-open'
    | 'my-active'
    | 'all-active'
    | 'all-tickets'
    | 'unassigned'
    | 'overdue'
    | 'triage'
    | 'calendar'
    | 'dashboard-created'
    | 'dashboard-resolved'
    | 'dashboard-open'
  /** English fallback labels, kept for pre-i18n call sites and as
   * the `fallback` argument to `translate()` so the UI never blanks
   * if the Fluent bundle hasn't initialised yet. */
  name: string
  description: string
  /** Fluent keys resolved at render time so locale switches reflect
   * without re-creating the constants module. */
  nameKey: string
  descriptionKey: string
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
  nameKey: 'builtin-view-my-open-name',
  descriptionKey: 'builtin-view-my-open-description',
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

/** Your in-flight work: tickets assigned to you that aren't done
 * or cancelled. Where MY_OPEN shows everything that bears your
 * name (including resolved tickets you're still on the hook to
 * verify), MY_ACTIVE narrows to the things you're actually
 * working right now. Mirrors Linear's "My Issues -> Active" and
 * Jira's "Assigned to me" agile filter. */
export const MY_ACTIVE_VIEW: BuiltInView = {
  id: 'my-active',
  name: 'My Active',
  description: 'Unresolved tickets assigned to you',
  nameKey: 'builtin-view-my-active-name',
  descriptionKey: 'builtin-view-my-active-description',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    predicate: {
      combinator: 'AND',
      children: [
        { field: 'workflow_state.category', op: 'not_in', value: ['done', 'cancelled'] },
      ],
    },
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
  nameKey: 'builtin-view-all-active-name',
  descriptionKey: 'builtin-view-all-active-description',
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

/** The unfiltered firehose: every ticket in scope regardless of
 * status, including done / cancelled / merged. The place you go
 * to search history, audit an old resolution, or confirm nothing
 * fell through the cracks. Empty predicate + no quick filters so
 * the header chip filters operate over the full corpus. */
export const ALL_TICKETS_VIEW: BuiltInView = {
  id: 'all-tickets',
  name: 'All Tickets',
  description: 'Every ticket, including resolved and cancelled',
  nameKey: 'builtin-view-all-tickets-name',
  descriptionKey: 'builtin-view-all-tickets-description',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    predicate: { combinator: 'AND', children: [] },
    quick_filters: [],
  },
}

/** Dispatch queue: active tickets that nobody owns yet. The
 * primary surface for a team lead or rotating dispatcher who
 * decides who picks up what. Pairs the `unassigned` quick filter
 * (assignee is null) with the active-status predicate so closed
 * tickets that were never assigned don't clutter the queue. */
export const UNASSIGNED_VIEW: BuiltInView = {
  id: 'unassigned',
  name: 'Unassigned',
  description: 'Active tickets with no assignee',
  nameKey: 'builtin-view-unassigned-name',
  descriptionKey: 'builtin-view-unassigned-description',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    predicate: {
      combinator: 'AND',
      children: [
        { field: 'workflow_state.category', op: 'not_in', value: ['done', 'cancelled'] },
      ],
    },
    quick_filters: ['unassigned'],
  },
}

/** Past-due watchlist: active tickets whose due_date has elapsed.
 * The `overdue` quick filter compares due_date against now; the
 * active-status predicate keeps resolved-but-late tickets out so
 * the list is actionable (things that are both late AND still
 * open) rather than a historical record of every missed date. */
export const OVERDUE_VIEW: BuiltInView = {
  id: 'overdue',
  name: 'Overdue',
  description: 'Active tickets past their due date',
  nameKey: 'builtin-view-overdue-name',
  descriptionKey: 'builtin-view-overdue-description',
  shape: { ...baseListShape, columns: defaultColumns },
  filter: {
    ...baseFilter,
    predicate: {
      combinator: 'AND',
      children: [
        { field: 'workflow_state.category', op: 'not_in', value: ['done', 'cancelled'] },
      ],
    },
    quick_filters: ['overdue'],
  },
}

export const TRIAGE_VIEW: BuiltInView = {
  id: 'triage',
  name: 'Triage',
  description: 'Tickets awaiting initial categorisation',
  nameKey: 'builtin-view-triage-name',
  descriptionKey: 'builtin-view-triage-description',
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
  nameKey: 'builtin-view-calendar-name',
  descriptionKey: 'builtin-view-calendar-description',
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
  MY_ACTIVE_VIEW,
  ALL_ACTIVE_VIEW,
  ALL_TICKETS_VIEW,
  UNASSIGNED_VIEW,
  OVERDUE_VIEW,
  TRIAGE_VIEW,
  CALENDAR_VIEW,
]

/** Dashboard KPI drill-down views. Not listed in the view switcher;
 * reached via `/tickets?view=dashboard-*` from dashboard widgets. */
export const DASHBOARD_BUILTIN_VIEWS: BuiltInView[] = [
  {
    id: 'dashboard-created',
    name: 'Created',
    description: 'Tickets created in the dashboard time range',
    nameKey: 'builtin-view-dashboard-created-name',
    descriptionKey: 'builtin-view-dashboard-created-description',
    shape: { ...baseListShape, columns: defaultColumns },
    filter: { ...baseFilter, predicate: { combinator: 'AND', children: [] }, quick_filters: [] },
  },
  {
    id: 'dashboard-resolved',
    name: 'Resolved',
    description: 'Tickets resolved in the dashboard time range',
    nameKey: 'builtin-view-dashboard-resolved-name',
    descriptionKey: 'builtin-view-dashboard-resolved-description',
    shape: { ...baseListShape, columns: defaultColumns },
    filter: { ...baseFilter, predicate: { combinator: 'AND', children: [] }, quick_filters: [] },
  },
  {
    id: 'dashboard-open',
    name: 'Open',
    description: 'Tickets that are not yet closed',
    nameKey: 'builtin-view-dashboard-open-name',
    descriptionKey: 'builtin-view-dashboard-open-description',
    shape: { ...baseListShape, columns: defaultColumns },
    filter: { ...baseFilter, predicate: { combinator: 'AND', children: [] }, quick_filters: [] },
  },
]

export function isDashboardBuiltinViewId(id: string): boolean {
  return DASHBOARD_BUILTIN_VIEWS.some((v) => v.id === id)
}

/** Apply dashboard time-window params from the URL to a drill-down
 * view filter so the list matches the KPI the user clicked. */
export function resolveDashboardViewFilter(
  viewId: string,
  from?: string,
  to?: string,
): FilterState {
  switch (viewId) {
    case 'dashboard-created':
      if (from && to) {
        return {
          ...baseFilter,
          predicate: {
            combinator: 'AND',
            children: [
              { field: 'created_at', op: 'gte', value: from },
              { field: 'created_at', op: 'lt', value: to },
            ],
          },
          quick_filters: [],
        }
      }
      return ALL_TICKETS_VIEW.filter
    case 'dashboard-resolved':
      if (from && to) {
        return {
          ...baseFilter,
          predicate: {
            combinator: 'AND',
            children: [
              { field: 'closed_at', op: 'is_not_empty', value: null },
              { field: 'closed_at', op: 'gte', value: from },
              { field: 'closed_at', op: 'lt', value: to },
            ],
          },
          quick_filters: [],
        }
      }
      return ALL_TICKETS_VIEW.filter
    case 'dashboard-open':
      return {
        ...baseFilter,
        predicate: {
          combinator: 'AND',
          children: [{ field: 'closed_at', op: 'is_empty', value: null }],
        },
        quick_filters: [],
      }
    default:
      return ALL_TICKETS_VIEW.filter
  }
}

export function findBuiltinView(id: string): BuiltInView | null {
  // Accept the legacy `my-queue` id so bookmarks / shared URLs
  // from before the rename keep working. Resolves to MY_OPEN_VIEW.
  if (id === 'my-queue') return MY_OPEN_VIEW
  return BUILTIN_VIEWS.find((v) => v.id === id)
    ?? DASHBOARD_BUILTIN_VIEWS.find((v) => v.id === id)
    ?? null
}
