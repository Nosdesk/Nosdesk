/**
 * Dashboard stats coordinator.
 *
 * Reads the user's active widgets from the layout store, collects
 * the union of `dataNeeds` declared in `widgets.ts`, and fires one
 * `/api/dashboard/stats?include=...` request via Pinia Colada.
 * Each stat widget injects the resulting handle and reads its own
 * slice — three widgets, one network request, one cache entry.
 *
 * Provided once in `DashboardView.vue` under `DASHBOARD_STATS_KEY`;
 * widgets retrieve via `useInjectedDashboardStats()`.
 */
import { computed, inject, type ComputedRef, type InjectionKey } from 'vue'
import { useQuery } from '@pinia/colada'
import { useAuthStore } from '@/stores/auth'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'
import { WIDGET_REGISTRY, type DashboardStatsGroup } from '@/views/dashboard/widgets'
import { getStats, type StatsBundle } from '@/services/dashboardService'

export interface DashboardStatsHandle {
  bundle: ComputedRef<StatsBundle | undefined>
  isLoading: ComputedRef<boolean>
  isError: ComputedRef<boolean>
}

export const DASHBOARD_STATS_KEY: InjectionKey<DashboardStatsHandle> =
  Symbol('dashboardStats')

/** Build the coordinator. Call exactly once at the dashboard root. */
export function useDashboardStats(): DashboardStatsHandle {
  const auth = useAuthStore()
  const layout = useDashboardLayoutStore()

  const userUuid = computed(() => auth.user?.uuid ?? '')

  // Union of dataNeeds across active (visible) widgets the user
  // can actually see. Sorted so the cache key is stable regardless
  // of widget order.
  const include = computed<DashboardStatsGroup[]>(() => {
    const visibleIds = new Set(
      layout.layout.widgets.filter((w) => w.visible).map((w) => w.id),
    )
    const needs = new Set<DashboardStatsGroup>()
    for (const def of WIDGET_REGISTRY) {
      if (!visibleIds.has(def.id)) continue
      for (const need of def.dataNeeds ?? []) needs.add(need)
    }
    return [...needs].sort()
  })

  const query = useQuery({
    key: () => ['dashboard', 'stats', userUuid.value, include.value.join(',')],
    query: () => getStats({ include: include.value, user: userUuid.value }),
    // Don't fire when there's nobody to fetch for or nothing to compute.
    enabled: () => !!userUuid.value && include.value.length > 0,
  })

  return {
    bundle: computed(() => query.data.value),
    isLoading: computed(() => query.asyncStatus.value === 'loading'),
    isError: computed(() => query.status.value === 'error'),
  }
}

/** Inject the coordinator inside a stat widget. Throws clearly if
 *  the widget is mounted outside a dashboard that provides it. */
export function useInjectedDashboardStats(): DashboardStatsHandle {
  const handle = inject(DASHBOARD_STATS_KEY)
  if (!handle) {
    throw new Error(
      'useInjectedDashboardStats: no DASHBOARD_STATS_KEY in scope. ' +
        'Provide via useDashboardStats() in the dashboard root.',
    )
  }
  return handle
}
