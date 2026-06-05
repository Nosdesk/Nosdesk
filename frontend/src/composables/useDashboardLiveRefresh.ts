/**
 * Dashboard-level live refresh.
 *
 * Reacts to the `sync_actions` change-stream rather than the discrete
 * `ticket-*` SSE events. The sync stream is delivered to every backend
 * machine via Postgres NOTIFY (the outbox), so the dashboard stays live
 * across fly machines, where the old discrete events were instance-local
 * and would have missed writes handled on another machine.
 *
 * One subscription serves the whole dashboard: on a debounced burst of
 * ticket mutations it invalidates the dashboard KPI query namespace
 * once, and Pinia Colada refetches whichever KPI widgets are mounted.
 *
 * Scope is `['dashboard', 'kpi']` per the dashboard plan's decision 9
 * ("live SSE on Row 1 KPIs only"). The headline KPIs derive from ticket
 * state, so we filter to the `ticket` aggregate. Widen the key or the
 * aggregate set if other widget families later opt into live refresh.
 *
 * Call once from the dashboard root (`DashboardView`).
 */
import { useQueryCache } from '@pinia/colada'
import { useSyncActions } from '@/composables/useSyncActions'

const DEBOUNCE_MS = 250

export function useDashboardLiveRefresh() {
  const cache = useQueryCache()
  useSyncActions(
    () => {
      void cache.invalidateQueries({ key: ['dashboard', 'kpi'] })
    },
    { aggregates: ['ticket'], debounceMs: DEBOUNCE_MS },
  )
}
