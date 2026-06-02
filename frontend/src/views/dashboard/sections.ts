/**
 * Canonical section anchors for the dashboard
 * (docs/dashboard-and-analytics-plan.md §6).
 *
 * Seven sections, in the order they appear on the canvas. The
 * AnchorRail reads this list to render its anchor links; Wave 8
 * seeds the canvas with H2 markers using the same ids.
 *
 * Reordering this list reorders the keyboard shortcuts (1-7) that
 * useDashboardKeybindings registers in a later wave. Be deliberate.
 */

export const SECTIONS: { id: string; labelKey: string }[] = [
  { id: 'today', labelKey: 'dashboard-section-today' },
  { id: 'volume-sla', labelKey: 'dashboard-section-volume-sla' },
  { id: 'queue-health', labelKey: 'dashboard-section-queue-health' },
  { id: 'agents', labelKey: 'dashboard-section-agents' },
  { id: 'categories', labelKey: 'dashboard-section-categories' },
  { id: 'backlog-ageing', labelKey: 'dashboard-section-backlog-ageing' },
  { id: 'audit-activity', labelKey: 'dashboard-section-audit-activity' },
]
