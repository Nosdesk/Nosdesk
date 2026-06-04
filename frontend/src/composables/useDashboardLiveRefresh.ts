/**
 * Dashboard-level live refresh.
 *
 * Secondary-subscriber pattern (see `useTicketActivitySSE` /
 * `useLowStockSSE`): owns no connection, only listeners. The shared
 * `EventSource` is connected app-wide by the notifications store, and
 * `ticket-*` events fan out on the global SSE topic, so the dashboard
 * just listens.
 *
 * One subscription serves the whole dashboard. On a burst of ticket
 * mutations it invalidates the dashboard KPI query namespace once;
 * Pinia Colada then refetches whichever KPI widgets are currently
 * mounted. This replaces the prior per-`KpiTile` subscriptions (each
 * tile carried its own listener set, debounce timer, and refetch),
 * which fired N refetches for one workspace-state change.
 *
 * Scope is `['dashboard', 'kpi']` per the dashboard plan's decision 9
 * ("live SSE on Row 1 KPIs only"; time-series / leaderboards /
 * heatmaps show "Updated HH:MM" with R-key refresh). Widen the key to
 * `['dashboard']` if other widget families later opt into live
 * refresh.
 *
 * Call once from the dashboard root (`DashboardView`).
 */
import { useQueryCache } from '@pinia/colada'
import { useSSEListeners } from '@/composables/useSSEListeners'

// Ticket mutations that move the headline KPI numbers. Over-inclusion
// is harmless: invalidation only refetches mounted queries, and the
// debounce collapses a multi-event burst into one refetch.
const LIVE_EVENTS = ['ticket-created', 'ticket-updated', 'ticket-deleted'] as const

const DEBOUNCE_MS = 250

export function useDashboardLiveRefresh() {
  const cache = useQueryCache()
  const { on, debouncedReload } = useSSEListeners({
    reload: () => {
      void cache.invalidateQueries({ key: ['dashboard', 'kpi'] })
    },
    debounceMs: DEBOUNCE_MS,
  })
  for (const event of LIVE_EVENTS) {
    on(event, debouncedReload)
  }
}
