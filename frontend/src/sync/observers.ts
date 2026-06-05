/**
 * Observer for applied sync actions.
 *
 * Imperative real-time consumers (activity-feed refetch, dashboard KPI
 * invalidation, toasts) subscribe here instead of the legacy discrete
 * per-event SSE channel (`ticket-updated`, `comment-added`, ...). The
 * observer is fed by `applySseFrame`, so it fires on the live, cross-
 * machine `sync_actions` stream (Postgres NOTIFY -> outbox -> SSE). That
 * is what makes these reactions correct across multiple backend machines
 * without any per-event broadcast bridge.
 *
 * Reactive *data* consumers do NOT need this: they bind to the object
 * pool (`useEntity` / `useAggregate`), which the same stream already
 * updates. Use this only for side effects that aren't expressible as
 * "render the current pool state".
 *
 * Not fired on initial hydrate: that is a bulk load, not a live change.
 */
import { logger } from '@/utils/logger'
import type { SyncAction } from './types'

type SyncActionsHandler = (actions: SyncAction[]) => void

const handlers = new Set<SyncActionsHandler>()

/** Subscribe to applied sync-action batches. Returns an unsubscribe fn. */
export function onSyncActions(handler: SyncActionsHandler): () => void {
  handlers.add(handler)
  return () => {
    handlers.delete(handler)
  }
}

/**
 * Fan a freshly-applied batch out to observers. Called by the sync
 * lifecycle after a live SSE frame is applied to the pool. An observer
 * throwing must never break the sync pipeline, so each is isolated.
 */
export function notifySyncActions(actions: SyncAction[]): void {
  if (handlers.size === 0 || actions.length === 0) return
  for (const handler of handlers) {
    try {
      handler(actions)
    } catch (err) {
      logger.error('onSyncActions handler threw', { error: err })
    }
  }
}
