/**
 * Dashboard widget registry.
 *
 * A widget is a discrete, self-contained Vue component the user can
 * move, resize, or hide on their dashboard. Each entry describes what
 * the component is, which roles may use it, and its default column /
 * row spans on the 3-column lattice. Placement follows the gravity
 * model (see `packAnchored`): the stored layout carries list order
 * plus an anchor column per widget, and rows derive from compaction.
 *
 * Layouts stored on the user row reference widgets by `id`. The store
 * merges new registry entries at the tail of the stored order, so
 * shipping a new widget is a no-op for existing users.
 */
import { defineAsyncComponent } from 'vue'
import type { Component } from 'vue'
import type { DashboardLayout, UserRole } from '@nosdesk/core/types/user'
import { packGrid } from '@/composables/usePointerSortable'
import UserAssignedTickets from '@/components/UserAssignedTickets.vue'
import TicketHeatmap from '@/components/TicketHeatmap.vue'
import RecentlyViewedWidget from './RecentlyViewedWidget.vue'
import UnassignedQueueWidget from './UnassignedQueueWidget.vue'
import KnowledgeGapsWidget from './KnowledgeGapsWidget.vue'
import SlaHealthWidget from './SlaHealthWidget.vue'
import TicketVolumeWidget from './TicketVolumeWidget.vue'
import TicketFlowChart from './charts/TicketFlowChart.vue'
import HorizontalBar from './charts/HorizontalBar.vue'

// Opt-in widgets that never appear on a default landing layout (they
// are off until a user adds them via the picker, or, for
// SavedViewWidget, pins a saved view): load on demand so they don't
// ship in the eager dashboard chunk. Each splits into its own small
// chunk fetched only when actually placed. RecentlyViewed +
// UnassignedQueue stay statically imported above because the curated
// staff default (STAFF_VISIBLE) shows them on first paint, so
// async-loading them would flash a blank cell on landing.
const StarredDocsWidget = defineAsyncComponent(() => import('./StarredDocsWidget.vue'))
const MyDevicesWidget = defineAsyncComponent(() => import('./MyAssetsWidget.vue'))
const ChannelHealthWidget = defineAsyncComponent(() => import('./ChannelHealthWidget.vue'))
const SavedViewWidget = defineAsyncComponent(() => import('./SavedViewWidget.vue'))
const KpiTile = defineAsyncComponent(() => import('./charts/KpiTile.vue'))

/**
 * Synthetic widget id prefix for saved-view-backed widgets.
 * Layouts persist `saved_view:<uuid>` instead of a registry id;
 * `widgetById` recognises the prefix and synthesises a registry
 * entry on the fly that points at the shared SavedViewWidget shell.
 */
export const SAVED_VIEW_WIDGET_PREFIX = 'saved_view:'

export function isSavedViewWidgetId(id: string): boolean {
  return id.startsWith(SAVED_VIEW_WIDGET_PREFIX)
}

export function savedViewWidgetId(uuid: string): string {
  return `${SAVED_VIEW_WIDGET_PREFIX}${uuid}`
}

/** Preview illustration kind for the add-widget picker. Each maps to a
 *  small themed thumbnail in `WidgetPreview.vue` — an abstract read of
 *  the widget's shape, not a live render. */
export type WidgetPreviewKind = 'kpi' | 'kpi-rail' | 'area' | 'bars' | 'heatmap' | 'list' | 'status'

const PREVIEW_BY_ID: Record<string, WidgetPreviewKind> = {
  'assigned-tickets': 'list',
  'requested-tickets': 'list',
  'ticket-volume': 'kpi-rail',
  'tickets-created': 'kpi',
  'tickets-resolved': 'kpi',
  'tickets-open': 'kpi',
  'tickets-over-time': 'area',
  'volume-by-category': 'bars',
  'volume-by-priority': 'bars',
  'unassigned-queue': 'list',
  'recently-viewed': 'list',
  'starred-docs': 'list',
  'my-devices': 'list',
  'channel-health': 'status',
  'activity-heatmap': 'heatmap',
  'knowledge-gaps': 'list',
  'sla-health': 'kpi-rail',
}

/** Preview kind for a registry widget id (falls back to a list shape). */
export function widgetPreviewKind(id: string): WidgetPreviewKind {
  return PREVIEW_BY_ID[id] ?? 'list'
}

/** Preview kind for a saved view's `viz_type`. */
export function savedViewPreviewKind(vizType: string | undefined): WidgetPreviewKind {
  switch (vizType) {
    case 'kpi_tile':
      return 'kpi'
    case 'line':
      return 'area'
    case 'horizontal_bar':
      return 'bars'
    case 'heatmap':
      return 'heatmap'
    default:
      return 'list'
  }
}

function savedViewUuidFromId(id: string): string {
  return id.slice(SAVED_VIEW_WIDGET_PREFIX.length)
}

export type WidgetSpan = 1 | 2 | 3

/** Column count of the desktop lattice. The single source of truth
 *  for the grid width: the packer callers, anchor clamps, and the
 *  legacy-anchor derivation all read this. (The `grid-cols-3`
 *  Tailwind classes and the backend `0..=2` validator mirror it but
 *  can't share a JS constant.) */
export const LATTICE_COLS = 3

/** Anchor column on the desktop lattice. */
export type WidgetCol = 0 | 1 | 2

/** One stored layout entry. Field order is CANONICAL (id, visible,
 *  span, rowSpan, col, config): the layout store detects dirtiness
 *  and SSE echoes via JSON.stringify equality, so every code path
 *  that builds or rebuilds an entry must emit keys in this order. */
export type LayoutWidgetEntry = DashboardLayout['widgets'][number]

export interface WidgetDef {
  /** Stable identifier persisted in user layouts. */
  id: string
  /** FTL key for the human-readable title shown in the "add widget" picker. */
  titleKey: string
  /** FTL key for the one-line description shown next to the title in the picker. */
  descriptionKey: string
  /** The Vue component rendered for this widget. */
  component: Component
  /** Static props passed to the component when rendered. */
  props?: Record<string, unknown>
  /** Column span inside the 3-col dashboard grid. */
  span: WidgetSpan
  /**
   * Row span on the fixed-unit grid lattice (1, 2, or 3 row units).
   * The dashboard grid uses `grid-auto-rows: var(--dash-row-unit)` so
   * every row is the same height; a widget's pixel height is
   * `rowSpan` units (plus inter-unit gaps). When omitted it derives
   * from `naturalHeight`: compact widgets (KPI tiles, glance panels)
   * default to 1 unit, everything else to 2. Override explicitly for
   * a widget that needs a taller or shorter footprint than its
   * `naturalHeight` flag implies.
   */
  rowSpan?: WidgetSpan
  /**
   * Minimum column span the widget stays usable at. Resize gestures,
   * the context menu, and keyboard sizing all clamp to it, and
   * `effectiveSpanFor` clamps read-side so a stored layout below a
   * later-raised minimum needs no migration. Defaults to 1.
   */
  minSpan?: WidgetSpan
  /** Minimum row span, same contract as `minSpan`. Defaults to 1. */
  minRowSpan?: WidgetSpan
  /** Which roles may use this widget. */
  roles: UserRole[]
  /**
   * Whether the widget is visible by default. When a new widget is
   * added to the registry, existing users inherit this flag the first
   * time their layout is merged. Defaults to `true` when omitted —
   * set `false` for niche widgets users should opt in to via the
   * "Add widget" picker rather than having them appear unprompted.
   */
  defaultVisible?: boolean
  /**
   * When `true`, the widget renders at its natural content height and
   * opts out of the grid's row-stretch. Use for compact widgets (stat
   * rails, glance panels) that would show distracting empty space if
   * stretched to match a tall list sibling. The unused grid-cell
   * space below falls through to the dashboard background — no
   * empty-card chrome, so the uneven bottom reads as "this widget is
   * compact by design" rather than "this card isn't filling."
   */
  naturalHeight?: boolean
  /**
   * Optional list of dashboard-stats groups this widget consumes
   * (e.g., `['queue']`, `['yours']`, `['summary']`). The dashboard
   * coordinator collects the union across active widgets and
   * fires one `/api/dashboard/stats?include=...` request that
   * serves them all. Widgets that don't read shared stats omit
   * this field.
   */
  dataNeeds?: readonly DashboardStatsGroup[]
  /**
   * Global page-chrome elements this widget depends on. The
   * `DashboardView` page only renders a chrome element when at
   * least one visible widget on the active layout declares the
   * matching dependency. So a layout containing zero time-aware
   * widgets gets a clean header (no orphaned time-range chips
   * setting URL state nothing reads). Empty / omitted means the
   * widget needs no chrome state.
   *
   *   - `time-range`     reads `useTimeRange` (charts, KPI tiles)
   *   - `compare`        renders compare-to-prior overlay / KPI delta
   */
  chromeDependencies?: readonly ChromeDependency[]
  /**
   * When `true`, `WidgetFrame` wraps this widget in a
   * `DashboardWidgetShell` using `titleKey` for the title. Used
   * by simple chart widgets (KpiTile, LineChart, HorizontalBar)
   * that delegate loading + error state to the chart component
   * itself and only need a titled card around them.
   *
   * Default `false` so existing widgets that self-shell continue
   * working unchanged.
   */
  frameWraps?: boolean
  /**
   * CSS `aspect-ratio` (e.g. `'2 / 1'`) for a PLOTTED-chart widget whose body
   * has no intrinsic height (LineChart, heatmap). Passed to the shell so the
   * plot derives its height from width on the stacked mobile layout instead of
   * collapsing, and fills its row on the xl lattice. Omit for KPI tiles / lists
   * (they size to their content). See `DashboardWidgetShell.bodyAspect`.
   */
  bodyAspect?: string
}

/** Page-chrome elements that widgets may depend on. */
export type ChromeDependency = 'time-range' | 'compare'

/** Stat group keys the backend recognises in `?include=...`. Keep in
 *  sync with `StatsGroup` in `backend/src/repository/dashboard_stats.rs`. */
export type DashboardStatsGroup = 'queue' | 'yours' | 'summary' | 'knowledge_gaps'

export const WIDGET_REGISTRY: WidgetDef[] = [
  {
    id: 'assigned-tickets',
    titleKey: 'dashboard-widget-assigned-tickets-title',
    descriptionKey: 'dashboard-widget-assigned-tickets-description',
    component: UserAssignedTickets,
    props: { limit: 10 },
    span: 2,
    // 3 row units so the 10-item list isn't clipped at the default 2.
    rowSpan: 3,
    roles: ['technician', 'admin'],
  },
  {
    id: 'ticket-volume',
    titleKey: 'dashboard-widget-ticket-volume-title',
    descriptionKey: 'dashboard-widget-ticket-volume-description',
    component: TicketVolumeWidget,
    span: 2,
    roles: ['technician', 'admin'],
    naturalHeight: true,
    chromeDependencies: ['time-range', 'compare'],
  },
  {
    id: 'tickets-created',
    titleKey: 'dashboard-system-tickets-created-title',
    descriptionKey: 'dashboard-system-tickets-created-description',
    component: KpiTile,
    props: { metric: 'tickets_created', listViewId: 'dashboard-created' },
    span: 1,
    roles: ['technician', 'admin'],
    naturalHeight: true,
    chromeDependencies: ['time-range', 'compare'],
    frameWraps: true,
    defaultVisible: false,
  },
  {
    id: 'tickets-resolved',
    titleKey: 'dashboard-system-tickets-resolved-title',
    descriptionKey: 'dashboard-system-tickets-resolved-description',
    component: KpiTile,
    props: { metric: 'tickets_resolved', listViewId: 'dashboard-resolved' },
    span: 1,
    roles: ['technician', 'admin'],
    naturalHeight: true,
    chromeDependencies: ['time-range', 'compare'],
    frameWraps: true,
    defaultVisible: false,
  },
  {
    id: 'tickets-open',
    titleKey: 'dashboard-system-tickets-open-title',
    descriptionKey: 'dashboard-system-tickets-open-description',
    component: KpiTile,
    props: { metric: 'tickets_open', listViewId: 'dashboard-open' },
    span: 1,
    roles: ['technician', 'admin'],
    naturalHeight: true,
    chromeDependencies: ['time-range', 'compare'],
    frameWraps: true,
    defaultVisible: false,
  },
  {
    id: 'tickets-over-time',
    titleKey: 'dashboard-system-tickets-over-time-title',
    descriptionKey: 'dashboard-system-tickets-over-time-description',
    component: TicketFlowChart,
    span: 2,
    // A plotted time series at a single row unit is an unreadable
    // sliver; hold it at two.
    minRowSpan: 2,
    roles: ['technician', 'admin'],
    // Two series already provide the comparison; no prior overlay, so
    // the widget depends on the range chips but not the compare toggle.
    chromeDependencies: ['time-range'],
    frameWraps: true,
    bodyAspect: '2 / 1',
  },
  {
    id: 'volume-by-category',
    titleKey: 'dashboard-system-volume-by-category-title',
    descriptionKey: 'dashboard-system-volume-by-category-description',
    component: HorizontalBar,
    props: { groupBy: 'category', topN: 8 },
    span: 1,
    roles: ['technician', 'admin'],
    chromeDependencies: ['time-range'],
    frameWraps: true,
  },
  {
    id: 'volume-by-priority',
    titleKey: 'dashboard-system-volume-by-priority-title',
    descriptionKey: 'dashboard-system-volume-by-priority-description',
    component: HorizontalBar,
    props: { groupBy: 'priority' },
    span: 1,
    roles: ['technician', 'admin'],
    chromeDependencies: ['time-range'],
    frameWraps: true,
  },
  {
    id: 'unassigned-queue',
    titleKey: 'dashboard-widget-unassigned-queue-title',
    descriptionKey: 'dashboard-widget-unassigned-queue-description',
    component: UnassignedQueueWidget,
    span: 2,
    roles: ['technician', 'admin'],
    defaultVisible: false,
  },
  {
    id: 'recently-viewed',
    titleKey: 'dashboard-widget-recently-viewed-title',
    descriptionKey: 'dashboard-widget-recently-viewed-description',
    component: RecentlyViewedWidget,
    span: 1,
    roles: ['technician', 'admin', 'user'],
    defaultVisible: false,
  },
  {
    id: 'starred-docs',
    titleKey: 'dashboard-widget-starred-docs-title',
    descriptionKey: 'dashboard-widget-starred-docs-description',
    component: StarredDocsWidget,
    span: 1,
    roles: ['technician', 'admin', 'user'],
    defaultVisible: false,
  },
  {
    id: 'my-devices',
    titleKey: 'dashboard-widget-my-devices-title',
    descriptionKey: 'dashboard-widget-my-devices-description',
    component: MyDevicesWidget,
    span: 1,
    roles: ['technician', 'admin', 'user'],
    defaultVisible: false,
  },
  {
    id: 'channel-health',
    titleKey: 'dashboard-widget-channel-health-title',
    descriptionKey: 'dashboard-widget-channel-health-description',
    component: ChannelHealthWidget,
    span: 1,
    roles: ['admin'],
    defaultVisible: false,
  },
  {
    id: 'activity-heatmap',
    titleKey: 'dashboard-widget-activity-heatmap-title',
    descriptionKey: 'dashboard-widget-activity-heatmap-description',
    component: TicketHeatmap,
    props: { mode: 'completed', titleKey: 'dashboard-widget-activity-heatmap-prop-title' },
    span: 3,
    roles: ['technician', 'admin'],
  },
  {
    id: 'requested-tickets',
    titleKey: 'dashboard-widget-requested-tickets-title',
    descriptionKey: 'dashboard-widget-requested-tickets-description',
    component: UserAssignedTickets,
    props: { ticketType: 'requested', limit: 10, titleKey: 'dashboard-widget-requested-tickets-prop-title' },
    span: 2,
    roles: ['user'],
  },
  {
    id: 'knowledge-gaps',
    titleKey: 'dashboard-widget-knowledge-gaps-title',
    descriptionKey: 'dashboard-widget-knowledge-gaps-description',
    component: KnowledgeGapsWidget,
    span: 1,
    roles: ['technician', 'admin'],
    dataNeeds: ['knowledge_gaps'],
  },
  {
    id: 'sla-health',
    titleKey: 'dashboard-widget-sla-health-title',
    descriptionKey: 'dashboard-widget-sla-health-description',
    component: SlaHealthWidget,
    span: 1,
    roles: ['technician', 'admin'],
    naturalHeight: true,
  },
]

/** Return widget definitions available to the given role. */
export function widgetsForRole(role: UserRole): WidgetDef[] {
  return WIDGET_REGISTRY.filter((w) => w.roles.includes(role))
}

/**
 * Curated default layout for technicians and admins.
 *
 * Anchor columns (`col`) place the mosaic explicitly; rows derive
 * from the gravity packer (each widget floats up in its column band).
 * The anchors reproduce what the legacy dense auto-flow produced from
 * this order, so long-time users see no change. Heights carry the
 * structure: Assigned Tickets is a tall column (3 row units) on the
 * left; the time chart and Recently Viewed run 2 units; the activity
 * heatmap spans 2 columns as a wide band; everything else is a
 * compact 1-unit tile.
 *
 * KPI / chart widgets read the global time range and declare
 * `chromeDependencies`, so the DashboardView only renders the
 * time-range chrome when one of them is in the visible set.
 *
 * Widgets not listed are appended at the tail hidden, so they show
 * up in the "Add widget" picker without cluttering the initial view.
 */
const STAFF_VISIBLE: DashboardLayout['widgets'] = [
  { id: 'assigned-tickets', visible: true, span: 1, rowSpan: 3, col: 0 },
  { id: 'ticket-volume', visible: true, span: 1, rowSpan: 1, col: 1 },
  { id: 'sla-health', visible: true, span: 1, col: 2 },
  { id: 'unassigned-queue', visible: true, span: 1, col: 1 },
  { id: 'tickets-over-time', visible: true, span: 1, rowSpan: 2, col: 2 },
  { id: 'recently-viewed', visible: true, span: 1, rowSpan: 2, col: 0 },
  { id: 'activity-heatmap', visible: true, span: 2, rowSpan: 1, col: 1 },
  { id: 'volume-by-priority', visible: true, span: 1, rowSpan: 1, col: 1 },
  { id: 'volume-by-category', visible: true, span: 1, rowSpan: 1, col: 2 },
]

/** The default layout for a role. Staff roles (technician / admin)
 * get a curated ordering with spans + Queue-metric defaults. Regular
 * users get the registry-order fallback since their widget set is
 * small enough that a hand-curated layout adds no value. */
export function defaultLayoutFor(role: UserRole): DashboardLayout {
  if (role === 'technician' || role === 'admin') {
    const available = new Set(widgetsForRole(role).map((w) => w.id))
    const visible = STAFF_VISIBLE.filter((w) => available.has(w.id))
    const visibleIds = new Set(visible.map((w) => w.id))
    const hidden = widgetsForRole(role)
      .filter((w) => !visibleIds.has(w.id))
      .map((w) => ({ id: w.id, visible: false }))
    return { widgets: [...visible, ...hidden] }
  }
  return {
    widgets: widgetsForRole(role).map((w) => ({
      id: w.id,
      visible: w.defaultVisible ?? true,
    })),
  }
}

/**
 * Reconcile a stored layout against the current registry for the
 * given role: drop unknown ids, drop ids the role cannot use, and
 * append any newly-registered widgets at the tail with `visible: true`.
 * Always returns a well-formed layout the UI can render without extra
 * null checks.
 *
 * When the user has no stored layout at all, we seed from the role's
 * curated `defaultLayoutFor(role)` instead of raw registry order —
 * otherwise first-time users would land on a layout that doesn't
 * match what "Reset to defaults" produces.
 */
/**
 * True when a layout entry's id resolves to something this role
 * can render. Leans on `widgetById` (the single source of truth for
 * id resolution) so both static registry ids AND synthetic
 * `saved_view:<uuid>` ids stay in the layout through the merge.
 * Without this, the saved-view widget the user just pinned would
 * be silently dropped on the next load — `widgetsForRole` only
 * lists the static registry.
 */
function isAvailableForRole(id: string, role: UserRole): boolean {
  const def = widgetById(id)
  return def != null && def.roles.includes(role)
}

const LEGACY_TICKET_KPI_IDS = ['tickets-created', 'tickets-resolved', 'tickets-open'] as const

/** Replace the three legacy single-metric KPI tiles with the grouped
 *  ticket-volume widget when a stored layout still carries the old
 *  default row. Users who split them intentionally (only 1–2 present)
 *  are left alone. */
function migrateLegacyTicketKpis(
  widgets: DashboardLayout['widgets'],
): DashboardLayout['widgets'] {
  const ids = new Set(widgets.map((w) => w.id))
  if (ids.has('ticket-volume')) return widgets
  if (!LEGACY_TICKET_KPI_IDS.every((id) => ids.has(id))) return widgets

  const legacy = new Set<string>(LEGACY_TICKET_KPI_IDS)
  const firstLegacyIdx = widgets.findIndex((w) => legacy.has(w.id))
  if (firstLegacyIdx < 0) return widgets

  const hidden = widgets.map((w) =>
    legacy.has(w.id) ? { ...w, visible: false } : w,
  )
  return [
    ...hidden.slice(0, firstLegacyIdx),
    { id: 'ticket-volume', visible: true, span: 2 as WidgetSpan },
    ...hidden.slice(firstLegacyIdx),
  ]
}

export function mergeWithRegistry(
  stored: DashboardLayout | null | undefined,
  role: UserRole,
): DashboardLayout {
  if (!stored?.widgets?.length) {
    return defaultLayoutFor(role)
  }
  const base = stored.widgets
  const seen = new Set<string>()
  const kept = base
    .filter((e) => !seen.has(e.id) && isAvailableForRole(e.id, role))
    .map((e) => {
      seen.add(e.id)
      // Keys emitted in the canonical entry order (see
      // LayoutWidgetEntry); JSON.stringify equality depends on it.
      const entry: LayoutWidgetEntry = {
        id: e.id,
        visible: !!e.visible,
      }
      if (e.span === 1 || e.span === 2 || e.span === 3) {
        entry.span = e.span
      }
      if (e.rowSpan === 1 || e.rowSpan === 2 || e.rowSpan === 3) {
        entry.rowSpan = e.rowSpan
      }
      if (e.col === 0 || e.col === 1 || e.col === 2) {
        entry.col = e.col
      }
      if (e.config && typeof e.config === 'object' && !Array.isArray(e.config)) {
        entry.config = e.config as Record<string, unknown>
      }
      return entry
    })
  const missing = widgetsForRole(role)
    .filter((w) => !seen.has(w.id))
    .map((w) => ({ id: w.id, visible: w.defaultVisible ?? true }))
  return {
    widgets: deriveLegacyAnchors(migrateLegacyTicketKpis([...kept, ...missing])),
  }
}

/**
 * One-time anchor derivation for layouts saved before anchor columns
 * existed. Runs ONLY when no visible entry carries a `col`: the
 * legacy dense packer (`packGrid`, byte-identical to the CSS dense
 * flow those users were seeing) assigns each visible widget the
 * column it already renders in, so the upgrade changes nothing on
 * screen. Mixed layouts (some entries with `col`, e.g. after a new
 * widget ships or the picker adds one) are left alone; col-less
 * entries pack as auto until the next placement commit materializes
 * them.
 */
function deriveLegacyAnchors(
  widgets: DashboardLayout['widgets'],
): DashboardLayout['widgets'] {
  const visible = widgets
    .map((w, i) => ({ w, i }))
    .filter(({ w }) => w.visible && widgetById(w.id))
  if (visible.length === 0) return widgets
  if (visible.some(({ w }) => w.col != null)) return widgets
  const cells = packGrid(
    visible.map(({ w, i }) => ({
      originalIndex: i,
      colSpan: effectiveSpanFor(w),
      rowSpan: rowSpanFor(w),
    })),
    LATTICE_COLS,
  )
  return widgets.map((w, i) => {
    const cell = cells.get(i)
    if (!cell) return w
    return withEntryCol(w, cell.col as WidgetCol)
  })
}

/** Rebuild an entry with `col` set, emitting keys in the canonical
 *  order so JSON.stringify comparisons (isDirty, SSE echo) stay
 *  stable no matter which path wrote the entry. */
export function withEntryCol(entry: LayoutWidgetEntry, col: WidgetCol): LayoutWidgetEntry {
  const out: LayoutWidgetEntry = { id: entry.id, visible: entry.visible }
  if (entry.span != null) out.span = entry.span
  if (entry.rowSpan != null) out.rowSpan = entry.rowSpan
  out.col = col
  if (entry.config != null) out.config = entry.config
  return out
}

/** Look up a widget def by id. Returns undefined for unknown ids.
 *  Recognises the synthetic `saved_view:<uuid>` prefix by synthesising
 *  a registry entry that points at the shared SavedViewWidget shell
 *  with the uuid threaded through as a prop — saved-view widgets
 *  share one component definition so they don't bloat the static
 *  registry. */
export function widgetById(id: string): WidgetDef | undefined {
  if (isSavedViewWidgetId(id)) {
    return {
      id,
      titleKey: 'dashboard-widget-saved-view-title',
      descriptionKey: 'dashboard-widget-saved-view-description',
      component: SavedViewWidget,
      props: { viewUuid: savedViewUuidFromId(id) },
      span: 1,
      roles: ['technician', 'admin', 'user'],
      defaultVisible: false,
    }
  }
  return WIDGET_REGISTRY.find((w) => w.id === id)
}

/** Minimum column span for a widget id. Goes through `widgetById`
 *  so synthetic saved-view ids resolve too. */
export function minSpanFor(id: string): WidgetSpan {
  return widgetById(id)?.minSpan ?? 1
}

/** Minimum row span for a widget id. */
export function minRowSpanFor(id: string): WidgetSpan {
  return widgetById(id)?.minRowSpan ?? 1
}

/** Effective column span for a stored layout entry: user override
 *  (set via the resize control) wins, else the registry default.
 *  Clamped to the registry minimum so a raised minimum takes effect
 *  on stored layouts without a migration. */
export function effectiveSpanFor(entry: { id: string; span?: WidgetSpan }): WidgetSpan {
  const span = entry.span ?? widgetById(entry.id)?.span ?? 1
  return Math.max(span, minSpanFor(entry.id)) as WidgetSpan
}

/** Tailwind class for a widget's row span on the BELOW-xl lattice.
 *
 * At xl the grid places every widget explicitly (inline
 * `grid-column` / `grid-row` custom properties computed by the
 * gravity packer), so no span class may apply there: `row-span-2`
 * and the arbitrary `xl:[grid-row:...]` property are both single
 * classes and their winner would depend on stylesheet order.
 *
 * Below xl, edit mode keeps the fixed 1-column lattice (the drag
 * projection reads the fixed row unit), so the span renders via
 * `max-xl:`. View mode below xl flows at content height (capped by
 * a max-height on the frame) and needs no class at all. */
export function rowSpanClass(span: WidgetSpan, editMode: boolean): string {
  if (!editMode) return ''
  switch (span) {
    case 1:
      return 'max-xl:row-span-1'
    case 2:
      return 'max-xl:row-span-2'
    case 3:
      return 'max-xl:row-span-3'
  }
}

/** Effective anchor column for a stored layout entry: the user's
 *  saved anchor clamped so the given span still fits the 3-column
 *  lattice, or undefined for auto (earliest free slot). */
export function effectiveColFor(
  entry: { id: string; span?: WidgetSpan; col?: WidgetCol },
  span: WidgetSpan = effectiveSpanFor(entry),
): WidgetCol | undefined {
  if (entry.col == null) return undefined
  return Math.max(0, Math.min(LATTICE_COLS - span, entry.col)) as WidgetCol
}

/** Effective row span for a stored layout entry. Precedence: the
 *  user's saved override (set by the corner-resize handle) > the
 *  registry's explicit `rowSpan` > derived from `naturalHeight`
 *  (compact widgets 1 unit, lists/charts 2). The `naturalHeight` flag
 *  already partitions short from tall, so widgets need no data change. */
export function rowSpanFor(entry: { id: string; rowSpan?: WidgetSpan }): WidgetSpan {
  const min = minRowSpanFor(entry.id)
  if (entry.rowSpan != null) return Math.max(entry.rowSpan, min) as WidgetSpan
  const def = widgetById(entry.id)
  if (def?.rowSpan) return Math.max(def.rowSpan, min) as WidgetSpan
  return Math.max(def?.naturalHeight ? 1 : 2, min) as WidgetSpan
}
