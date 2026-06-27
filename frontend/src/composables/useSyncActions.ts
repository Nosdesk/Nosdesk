/**
 * React to live sync-action batches with component lifecycle.
 *
 * The idiomatic replacement for subscribing to discrete semantic SSE
 * events (`ticket-updated`, `comment-added`, ...). Those are
 * instance-local; the sync-action stream is delivered to every backend
 * machine via Postgres NOTIFY, so a handler wired here is correct across
 * fly machines for free.
 *
 * Use for imperative side effects (refetch, cache-invalidate, toast).
 * For reactive data, bind to the sync object pool instead
 * (`useEntity` / `useAggregate`), which the same stream already updates.
 *
 * Usage:
 *   useSyncActions(() => query.refetch(), { aggregates: ['ticket'], debounceMs: 250 })
 */
import { onMounted, onUnmounted } from 'vue'
import { onSyncActions } from '@nosdesk/core/sync/observers'
import type { SyncAction, SyncAggregate } from '@nosdesk/core/sync/types'

interface UseSyncActionsOptions {
  /** Only fire for actions on these aggregates. Omit to receive all. */
  aggregates?: SyncAggregate[]
  /** Coalesce a burst into one trailing call after this many ms.
   *  Default 0 fires synchronously per applied batch. */
  debounceMs?: number
}

export function useSyncActions(
  handler: (actions: SyncAction[]) => void,
  options: UseSyncActionsOptions = {},
) {
  const { aggregates, debounceMs = 0 } = options
  const aggregateSet = aggregates && aggregates.length ? new Set(aggregates) : null

  let unsubscribe: (() => void) | null = null
  let timer: ReturnType<typeof setTimeout> | null = null
  let pending: SyncAction[] = []

  function flush() {
    timer = null
    const batch = pending
    pending = []
    if (batch.length) handler(batch)
  }

  function onActions(actions: SyncAction[]) {
    const relevant = aggregateSet
      ? actions.filter((a) => aggregateSet.has(a.aggregate))
      : actions
    if (relevant.length === 0) return

    if (debounceMs <= 0) {
      handler(relevant)
      return
    }
    pending.push(...relevant)
    if (timer) clearTimeout(timer)
    timer = setTimeout(flush, debounceMs)
  }

  onMounted(() => {
    unsubscribe = onSyncActions(onActions)
  })

  onUnmounted(() => {
    unsubscribe?.()
    unsubscribe = null
    if (timer) clearTimeout(timer)
  })
}
