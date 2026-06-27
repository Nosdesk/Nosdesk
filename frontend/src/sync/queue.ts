/**
 * Optimistic transaction queue.
 *
 * Writes go to the pool immediately, get persisted to IndexedDB,
 * then flushed to /api/sync/push in background. On rejection the
 * inverse patch reverts the optimistic apply and the user is
 * notified.
 *
 * Persistence-before-network is the crash safety guarantee: a
 * tab crash or refresh between optimistic apply and POST returns
 * to a state where the in-memory pool has the optimistic value
 * AND the IDB queue has the pending tx ready to be retried on
 * next boot.
 */
import { logger } from '@nosdesk/core/utils/logger'
import { apiBaseUrl, transport } from '@nosdesk/core/transport'
import { workspaceHeaders } from '@/services/activeWorkspace'
import * as pool from '@nosdesk/core/sync/pool'
import * as idb from './idb'
import type { PushResponse, PushTransaction, SyncAggregate } from '@nosdesk/core/sync/types'

let handle: idb.IdbHandle | null = null
let flushing = false
/**
 * Wall-clock backoff for the next flush attempt after a network
 * failure. Reset to 0 on every successful flush. Capped at 30s so
 * a long outage doesn't push retries to the next ice age.
 */
let backoffMs = 0
const BACKOFF_INITIAL_MS = 500
const BACKOFF_MAX_MS = 30_000

/** Max transactions per push request — matches the backend cap so a
 * batch never gets rejected on size alone. */
const PUSH_BATCH_LIMIT = 50

export function setIdbHandle(h: idb.IdbHandle | null): void {
  handle = h
}

interface QueuedTx extends PushTransaction {
  createdAt: number
  /** Patch that reverses the optimistic apply if the server rejects
   * the forward patch. Computed at dispatch time from the row's
   * pre-mutation state. */
  inverse?: Record<string, unknown>
}

/**
 * One forward+inverse pair on a single aggregate row. The
 * dispatcher applies `forward` to the pool, sends `forward` to the
 * server, and falls back to `inverse` on rejection.
 */
export interface OptimisticPatch<T extends object> {
  forward: Partial<T>
  inverse: Partial<T>
}

/**
 * Generate a fresh transaction id. Server only requires it to be a
 * non-empty unique string; UUIDv4 from crypto.randomUUID() is more
 * than enough — its 122 random bits are a sufficient idempotency
 * key against client retries, and we avoid pulling in `ulid` for
 * its time-sortable property (the server's monotonic sync_id
 * already provides global ordering).
 */
function freshTxId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  // Fallback for environments without WebCrypto. The collision risk
  // here is bounded by Date.now() resolution + Math.random() entropy
  // — fine for a dev branch but the production target is browsers
  // with crypto.randomUUID() (every browser since 2022).
  return `t-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

/**
 * Strip a patch to a plain, structured-clone-safe object graph.
 *
 * Patch values frequently carry Vue `reactive()` proxies: a status
 * change copies the denormalised `workflow_state` object straight
 * out of the workflow-states Pinia store, and the inverse copies it
 * from the reactive pool row. IndexedDB persists transactions via
 * the structured-clone algorithm, which throws `DataCloneError` on
 * those proxies ("could not be cloned"). `toRaw()` only unwraps the
 * top-level proxy, not the nested objects a patch embeds, so we do a
 * JSON round-trip: it's lossless for patch payloads (they're POSTed
 * to /api/sync/push as JSON anyway) and produces a proxy-free graph
 * that both IndexedDB and the pool accept.
 */
function plainSnapshot<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

/**
 * Apply an optimistic patch to a row, persist the transaction, and
 * schedule a flush. The pool sees the change immediately; the
 * server hears about it on the next flush tick.
 *
 * Returns the transaction id so callers can correlate with
 * subsequent rejection notifications.
 */
export async function dispatchOptimistic<T extends object>(
  aggregate: SyncAggregate,
  modelId: string | number,
  patch: OptimisticPatch<T>,
): Promise<string | null> {
  const current = pool.get<T>(aggregate, modelId)
  if (!current) return null

  // Snapshot to proxy-free plain objects before they touch
  // IndexedDB (see `plainSnapshot`). The forward patch is also what
  // we apply to the pool below, so using the snapshot keeps the pool
  // from retaining references back into store-owned reactive state.
  const forward = plainSnapshot(patch.forward) as Record<string, unknown>
  const inverse = plainSnapshot(patch.inverse) as Record<string, unknown>

  const tx: QueuedTx = {
    tx_id: freshTxId(),
    aggregate,
    model_id: String(modelId),
    op: 'U',
    patch: forward,
    base_sync_id: pool.getLastSyncId(),
    inverse,
    createdAt: Date.now(),
  }

  // Persist BEFORE applying the optimistic mutation so a crash
  // between the two steps doesn't leave the pool optimistic with
  // no pending tx to flush.
  if (handle) {
    try {
      await idb.putTransaction(handle, tx)
    } catch (e) {
      logger.error('Failed to persist optimistic tx; aborting dispatch', { error: e })
      return null
    }
  }
  pool.patch(aggregate, modelId, forward as Partial<T>)
  void scheduleFlush()
  return tx.tx_id
}

/**
 * Drain pending transactions to the server. Runs at most one
 * flush at a time (subsequent calls are no-ops while in-flight).
 * Network failures schedule a backoff retry; per-tx rejections
 * apply the inverse patch and surface a notification.
 */
export async function flush(): Promise<void> {
  if (flushing || !handle) return
  flushing = true
  try {
    while (true) {
      const all = await idb.loadTransactions(handle)
      if (all.length === 0) break

      const batch = all.slice(0, PUSH_BATCH_LIMIT)
      const wirePayload: PushTransaction[] = batch.map((q) => ({
        tx_id: q.tx_id,
        aggregate: q.aggregate,
        model_id: q.model_id,
        op: q.op,
        patch: q.patch,
        base_sync_id: q.base_sync_id ?? null,
      }))

      let response: PushResponse | null = null
      try {
        // Raw fetch (not apiClient) by design, so resolve base URL, auth
        // headers (the global CSRF middleware requires the double-submit
        // header on this POST), the selection header (empty in host mode),
        // and credential mode from the transport seam directly.
        const headers: Record<string, string> = {
          'Content-Type': 'application/json',
          ...workspaceHeaders(),
          ...transport().auth.authHeaders(),
        }
        const res = await fetch(`${apiBaseUrl()}/sync/push`, {
          method: 'POST',
          headers,
          body: JSON.stringify(wirePayload),
          credentials: transport().auth.useCredentials ? 'include' : 'omit',
        })
        if (!res.ok) {
          throw new Error(`push failed: ${res.status}`)
        }
        response = (await res.json()) as PushResponse
      } catch (e) {
        logger.warn('sync push network failure; backing off', { error: e })
        scheduleRetry()
        return
      }

      backoffMs = 0
      // A push does not advance the read cursor. The read cursor is the
      // commit-safe `(xid8, sync_id)` pair owned by the bootstrap / delta
      // / SSE streams; a push only knows its own rows' `sync_id`, not
      // their `xid8`, so it can't form a valid composite cursor. The
      // pushed rows echo back through SSE with their xid8 and apply
      // idempotently (the optimistic write already populated the pool).

      for (const txId of response.applied) {
        await idb.deleteTransaction(handle, txId)
      }
      for (const r of response.rejected) {
        const tx = batch.find((b) => b.tx_id === r.tx_id)
        if (tx?.inverse) {
          pool.patch(
            tx.aggregate,
            tx.model_id,
            tx.inverse as Partial<Record<string, unknown>>,
          )
        }
        await idb.deleteTransaction(handle, r.tx_id)
        logger.warn(`sync push rejected: ${r.reason}`, {
          tx_id: r.tx_id,
          detail: r.detail,
        })
      }
    }
  } finally {
    flushing = false
  }
}

let flushTimer: ReturnType<typeof setTimeout> | null = null

function scheduleFlush(): Promise<void> {
  if (flushTimer != null) return Promise.resolve()
  flushTimer = setTimeout(() => {
    flushTimer = null
    void flush()
  }, 0)
  return Promise.resolve()
}

function scheduleRetry(): void {
  backoffMs = Math.min(Math.max(backoffMs * 2, BACKOFF_INITIAL_MS), BACKOFF_MAX_MS)
  if (flushTimer != null) clearTimeout(flushTimer)
  flushTimer = setTimeout(() => {
    flushTimer = null
    void flush()
  }, backoffMs)
}

/** Number of pending transactions still waiting to flush. Used by
 * the dev panel and integration tests; production code shouldn't
 * branch on this. */
export async function pendingCount(): Promise<number> {
  if (!handle) return 0
  const all = await idb.loadTransactions(handle)
  return all.length
}
