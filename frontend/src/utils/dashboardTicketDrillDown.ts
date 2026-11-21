import type { LocationQuery, RouteLocationRaw } from 'vue-router'
import type { DashboardDrillDownScope } from '@nosdesk/core/sync/views/builtinViews'
import type { KpiMetric } from '@/services/analyticsService'

/** Built-in ticket-list view ids used by dashboard KPI drill-down. */
export const DASHBOARD_METRIC_VIEW_IDS = {
  tickets_created: 'dashboard-created',
  tickets_resolved: 'dashboard-resolved',
  tickets_open: 'dashboard-open',
} as const satisfies Record<KpiMetric, string>

export type DashboardMetricViewId =
  (typeof DASHBOARD_METRIC_VIEW_IDS)[keyof typeof DASHBOARD_METRIC_VIEW_IDS]

const TIME_SCOPED_VIEWS = new Set<DashboardMetricViewId>([
  'dashboard-created',
  'dashboard-resolved',
])

export function dashboardMetricViewId(metric: KpiMetric): DashboardMetricViewId {
  return DASHBOARD_METRIC_VIEW_IDS[metric]
}

/** Route to the ticket list with the built-in view + optional window
 *  and narrowing. */
export function buildDashboardTicketDrillDown(
  viewId: string,
  window?: { from: string; to: string },
  scope: DashboardDrillDownScope = {},
): RouteLocationRaw {
  const query: Record<string, string> = { view: viewId }
  if (window && TIME_SCOPED_VIEWS.has(viewId as DashboardMetricViewId)) {
    query.from = window.from
    query.to = window.to
  }
  if (scope.doneOnly) query.outcome = 'done'
  if (scope.creditedTo) query.credit = scope.creditedTo
  return { path: '/tickets', query }
}

/** The narrowing a drill-down URL carries (see `buildDashboardTicketDrillDown`). */
export function dashboardDrillDownScopeFromQuery(query: LocationQuery): DashboardDrillDownScope {
  return {
    doneOnly: query.outcome === 'done',
    creditedTo: typeof query.credit === 'string' && query.credit ? query.credit : undefined,
  }
}

export function buildDashboardMetricDrillDown(
  metric: KpiMetric,
  window?: { from: string; to: string },
): RouteLocationRaw {
  return buildDashboardTicketDrillDown(dashboardMetricViewId(metric), window)
}
