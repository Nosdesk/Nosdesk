/**
 * Dashboard widget registry.
 *
 * A widget is a discrete, self-contained Vue component that the user
 * can reorder or hide on their dashboard. Each entry describes what
 * the component is, which roles may use it, and the fixed column
 * span it occupies inside the 3-column grid. The preset span is
 * deliberate — this iteration is "reorder + show/hide," not free-form
 * resize; giving each widget a design-time span keeps layouts tidy
 * without introducing a grid library.
 *
 * Layouts stored on the user row reference widgets by `id`. The store
 * merges new registry entries at the tail of the stored order, so
 * shipping a new widget is a no-op for existing users.
 */
import type { Component } from 'vue'
import type { DashboardLayout, UserRole } from '@/types/user'
import UserAssignedTickets from '@/components/UserAssignedTickets.vue'
import TicketHeatmap from '@/components/TicketHeatmap.vue'
import StaffYoursStats from './StaffYoursStats.vue'
import StaffQueueStats from './StaffQueueStats.vue'
import UserSummaryStats from './UserSummaryStats.vue'
import RecentlyViewedWidget from './RecentlyViewedWidget.vue'
import UnassignedQueueWidget from './UnassignedQueueWidget.vue'
import StarredDocsWidget from './StarredDocsWidget.vue'
import MyDevicesWidget from './MyAssetsWidget.vue'
import ChannelHealthWidget from './ChannelHealthWidget.vue'
import KnowledgeGapsWidget from './KnowledgeGapsWidget.vue'
import SlaHealthWidget from './SlaHealthWidget.vue'
import SavedViewWidget from './SavedViewWidget.vue'

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

function savedViewUuidFromId(id: string): string {
  return id.slice(SAVED_VIEW_WIDGET_PREFIX.length)
}

export type WidgetSpan = 1 | 2 | 3

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
}

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
    roles: ['technician', 'admin'],
  },
  {
    id: 'stats-yours',
    titleKey: 'dashboard-widget-stats-yours-title',
    descriptionKey: 'dashboard-widget-stats-yours-description',
    component: StaffYoursStats,
    span: 1,
    roles: ['technician', 'admin'],
    naturalHeight: true,
    dataNeeds: ['yours'],
  },
  {
    id: 'stats-queue',
    titleKey: 'dashboard-widget-stats-queue-title',
    descriptionKey: 'dashboard-widget-stats-queue-description',
    component: StaffQueueStats,
    span: 1,
    roles: ['technician', 'admin'],
    naturalHeight: true,
    dataNeeds: ['queue'],
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
    props: { ticketStatus: 'closed', titleKey: 'dashboard-widget-activity-heatmap-prop-title' },
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
    id: 'stats-summary',
    titleKey: 'dashboard-widget-stats-summary-title',
    descriptionKey: 'dashboard-widget-stats-summary-description',
    component: UserSummaryStats,
    span: 1,
    roles: ['user'],
    naturalHeight: true,
    dataNeeds: ['summary'],
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
 *   Row 1 — at-a-glance metrics
 *     [ Your Counts (span 2)  |  Queue Counts (span 1) ]
 *   Row 2 — the working set, scanning left-to-right for what needs doing
 *     [ Unassigned Queue  |  Recently Viewed  |  Assigned Tickets ]
 *   Row 3 — context
 *     [ Activity Heatmap (span 3) ]
 *
 * Queue Counts starts with the four metrics that matter day-to-day
 * for a triage-style workflow (open / in-progress / high priority /
 * closed today) rather than the unconfigured default of just
 * [unassigned, all]. Users can still swap any of this via the
 * per-widget picker and the dashboard edit mode.
 *
 * Widgets not listed in `visible` are appended at the tail hidden,
 * so they show up in the "Add widget" picker without cluttering the
 * initial view.
 */
const STAFF_VISIBLE: DashboardLayout['widgets'] = [
  { id: 'stats-yours', visible: true, span: 2 },
  {
    id: 'stats-queue',
    visible: true,
    span: 1,
    config: { metrics: ['open', 'in-progress', 'high-priority', 'closed-today'] },
  },
  { id: 'sla-health', visible: true, span: 1 },
  { id: 'unassigned-queue', visible: true, span: 1 },
  { id: 'recently-viewed', visible: true, span: 1 },
  { id: 'assigned-tickets', visible: true, span: 1 },
  { id: 'activity-heatmap', visible: true, span: 3 },
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
export function mergeWithRegistry(
  stored: DashboardLayout | null | undefined,
  role: UserRole,
): DashboardLayout {
  if (!stored?.widgets?.length) {
    return defaultLayoutFor(role)
  }
  const available = new Set(widgetsForRole(role).map((w) => w.id))
  const base = stored.widgets
  const seen = new Set<string>()
  const kept = base
    .filter((e) => available.has(e.id) && !seen.has(e.id))
    .map((e) => {
      seen.add(e.id)
      const entry: {
        id: string
        visible: boolean
        span?: WidgetSpan
        config?: Record<string, unknown>
      } = {
        id: e.id,
        visible: !!e.visible,
      }
      if (e.span === 1 || e.span === 2 || e.span === 3) {
        entry.span = e.span
      }
      if (e.config && typeof e.config === 'object' && !Array.isArray(e.config)) {
        entry.config = e.config as Record<string, unknown>
      }
      return entry
    })
  const missing = widgetsForRole(role)
    .filter((w) => !seen.has(w.id))
    .map((w) => ({ id: w.id, visible: w.defaultVisible ?? true }))
  return { widgets: [...kept, ...missing] }
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

/** Tailwind class for a widget's column span in the 3-col grid. */
export function spanClass(span: WidgetSpan): string {
  switch (span) {
    case 1:
      return 'xl:col-span-1'
    case 2:
      return 'xl:col-span-2'
    case 3:
      return 'xl:col-span-3'
  }
}

/** Effective column span for a stored layout entry: user override
 *  (set via the resize control) wins, else the registry default. */
export function effectiveSpanFor(entry: { id: string; span?: WidgetSpan }): WidgetSpan {
  return entry.span ?? widgetById(entry.id)?.span ?? 1
}
