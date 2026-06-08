import type { RouteLocationRaw } from 'vue-router'
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

/** Route to the ticket list with the built-in view + optional window. */
export function buildDashboardTicketDrillDown(
  viewId: string,
  window?: { from: string; to: string },
): RouteLocationRaw {
  const query: Record<string, string> = { view: viewId }
  if (window && TIME_SCOPED_VIEWS.has(viewId as DashboardMetricViewId)) {
    query.from = window.from
    query.to = window.to
  }
  return { path: '/tickets', query }
}

export function buildDashboardMetricDrillDown(
  metric: KpiMetric,
  window?: { from: string; to: string },
): RouteLocationRaw {
  return buildDashboardTicketDrillDown(dashboardMetricViewId(metric), window)
}
