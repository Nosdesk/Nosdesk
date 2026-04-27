/**
 * Aggregate "is the app doing async work right now?" by
 * inspecting Pinia Colada's query and mutation caches.
 *
 * Backs `<RouteProgress>` (top-bar progress indicator) and any
 * other UI that needs to know the global async pulse without
 * caring about specific keys.
 *
 * Implementation note: Pinia Colada exposes per-entry
 * `asyncStatus` as ShallowRefs. Iterating the cache Map inside
 * a `computed` reads each entry's status, so reactivity tracks
 * both Map mutations (entries added/removed) AND per-entry
 * status flips. No additional plumbing is needed.
 */
import { computed, type ComputedRef } from 'vue'
import { useMutationCache, useQueryCache } from '@pinia/colada'

export interface NetworkActivity {
  /** Number of queries + mutations currently in flight. */
  pendingCount: ComputedRef<number>
  /** True when at least one async op is pending. */
  hasPending: ComputedRef<boolean>
}

export function useNetworkActivity(): NetworkActivity {
  const queryCache = useQueryCache()
  const mutationCache = useMutationCache()

  const pendingCount = computed(() => {
    let count = 0
    for (const entry of queryCache.caches.values()) {
      if (entry.asyncStatus.value === 'loading') count++
    }
    for (const entry of mutationCache.caches.values()) {
      if (entry.asyncStatus.value === 'loading') count++
    }
    return count
  })

  return {
    pendingCount,
    hasPending: computed(() => pendingCount.value > 0),
  }
}
