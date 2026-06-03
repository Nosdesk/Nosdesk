/**
 * Section anchors for the dashboard
 * (docs/dashboard-and-analytics-plan.md §6).
 *
 * v1 ships the four sections whose widgets are actually present on
 * the canvas: Today (KPI tiles), Volume & SLA (the line chart),
 * Queue Health (the queue widgets), Agents (assigned tickets).
 *
 * Categories, Backlog & Ageing, Audit Activity are re-introduced in
 * parent-plan phases 5, 6 and 8 when their corresponding analytics
 * widgets land. Listing anchors for empty sections promises the
 * user content the dashboard can not yet deliver.
 *
 * Reordering this list reorders the keyboard shortcuts (1-N) that
 * useDashboardKeybindings registers. Be deliberate.
 */

export const SECTIONS: { id: string; labelKey: string }[] = [
  { id: 'today', labelKey: 'dashboard-section-today' },
  { id: 'volume-sla', labelKey: 'dashboard-section-volume-sla' },
  { id: 'queue-health', labelKey: 'dashboard-section-queue-health' },
  { id: 'agents', labelKey: 'dashboard-section-agents' },
]
