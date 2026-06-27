/**
 * Vue composables that read from the sync engine's object pool.
 *
 * `useEntity` returns a stable `computed` over `pool.get`. Because
 * the pool's root is a `reactive(Map)` and rows are
 * `shallowReactive`, the computed re-evaluates on row insertion,
 * deletion, and root-level field changes without any subscriber
 * registry.
 *
 * `useReference` is the same lookup with a side effect: missing
 * targets are debounced and fetched lazily through the (separately
 * implemented) reference-fetch dispatcher.
 */
import { computed, toValue, watchEffect, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import * as pool from './pool'
import type { SyncAggregate } from './types'

/**
 * Reactive lookup for a single row. Returns null when the id is
 * unset or when the row hasn't loaded yet — never throws. Pair
 * with `useReference` if you want the runtime to fetch the row on
 * demand instead of just returning null.
 */
export function useEntity<T extends object>(
  aggregate: SyncAggregate,
  id: MaybeRefOrGetter<string | number | null | undefined>,
): ComputedRef<T | null> {
  return computed<T | null>(() => {
    const v = toValue(id)
    if (v == null) return null
    return (pool.get<T>(aggregate, v) as T | undefined) ?? null
  })
}

/**
 * Lazy reference resolution: returns the same computed `useEntity`
 * does, but additionally schedules a fetch when the target id is
 * present in the pool. The fetcher is set by the lifecycle layer
 * via `setReferenceFetcher` so this module stays free of network
 * dependencies.
 */
export function useReference<T extends object>(
  aggregate: SyncAggregate,
  id: MaybeRefOrGetter<string | number | null | undefined>,
): ComputedRef<T | null> {
  const target = useEntity<T>(aggregate, id)
  watchEffect(() => {
    const v = toValue(id)
    if (v != null && !pool.has(aggregate, v)) {
      scheduleFetch(aggregate, String(v))
    }
  })
  return target
}

/**
 * Reactive iterable over every row of an aggregate. The returned
 * computed re-evaluates on insertion / deletion (membership
 * reactivity from the underlying reactive Map) and on root-level
 * field updates inside any of the iterated rows.
 *
 * Returns a fresh array on each evaluation so callers can sort /
 * filter without mutating the pool.
 */
export function useAggregate<T extends object>(
  aggregate: SyncAggregate,
): ComputedRef<T[]> {
  return computed(() => Array.from(pool.iterate<T>(aggregate) as IterableIterator<T>))
}

// -------------------- reference-fetch dispatcher --------------------
//
// `useReference` schedules a fetch when a referenced id isn't in the
// pool. The actual network call lives in the lifecycle module so
// this file stays free of HTTP concerns; the lifecycle layer calls
// `setReferenceFetcher` once at boot to wire the implementation.
//
// Pending requests are batched per animation frame so a screen
// rendering 200 references to lazy-loaded users issues one HTTP
// request, not 200.

type ReferenceFetcher = (aggregate: SyncAggregate, ids: string[]) => Promise<void>

let fetcher: ReferenceFetcher | null = null
const pendingByAggregate = new Map<SyncAggregate, Set<string>>()
let frameScheduled = false

export function setReferenceFetcher(fn: ReferenceFetcher | null): void {
  fetcher = fn
}

function scheduleFetch(aggregate: SyncAggregate, id: string): void {
  let bucket = pendingByAggregate.get(aggregate)
  if (!bucket) {
    bucket = new Set()
    pendingByAggregate.set(aggregate, bucket)
  }
  bucket.add(id)
  if (frameScheduled) return
  frameScheduled = true
  const tick = () => {
    frameScheduled = false
    if (!fetcher) return
    for (const [agg, ids] of pendingByAggregate) {
      if (ids.size === 0) continue
      const batch = Array.from(ids)
      ids.clear()
      void fetcher(agg, batch)
    }
  }
  if (typeof requestAnimationFrame === 'function') {
    requestAnimationFrame(tick)
  } else {
    setTimeout(tick, 16)
  }
}
