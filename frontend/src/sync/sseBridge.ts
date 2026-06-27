/**
 * Wires the existing SSE service's `sync-actions` event into the
 * sync engine's `applySseFrame`. One subscription per app instance,
 * registered at lifecycle bootstrap.
 *
 * The frame shape mirrors the Rust-side `SseEvent::SyncActions`:
 * `{ actions: SyncAction[], last_xid8: number, last_sync_id: number }`.
 * We accept `unknown` and validate at the boundary so a server typo
 * can't crash the sync engine.
 */
import { logger } from '@nosdesk/core/utils/logger'
import { useSSE } from '@/services/sseService'
import { unwrapEventData } from '@nosdesk/core/types/sse'
import { applySseFrame } from './lifecycle'
import type { SyncAction } from './types'

interface SyncActionsFrame {
  actions: SyncAction[]
  last_xid8: number
  last_sync_id: number
}

let attachedHandler: ((data: unknown) => void) | null = null

/**
 * Register the listener. Idempotent — calling twice replaces the
 * existing handler so a hot-reloaded module doesn't double-fire.
 */
export function attachSseBridge(): void {
  const sse = useSSE()
  if (attachedHandler) {
    sse.removeEventListener('sync-actions', attachedHandler)
  }
  const handler = (raw: unknown) => {
    const frame = parseFrame(raw)
    if (!frame) return
    applySseFrame(frame.actions, frame.last_xid8, frame.last_sync_id)
  }
  attachedHandler = handler
  sse.addEventListener('sync-actions', handler)
}

export function detachSseBridge(): void {
  if (!attachedHandler) return
  const sse = useSSE()
  sse.removeEventListener('sync-actions', attachedHandler)
  attachedHandler = null
}

function parseFrame(raw: unknown): SyncActionsFrame | null {
  if (!raw || typeof raw !== 'object') return null
  // `SseEvent` is an adjacently-tagged enum on the Rust side
  // (`#[serde(tag = "type", content = "data")]`), so the SyncActions
  // payload arrives wrapped as `{ type, data: { actions, last_sync_id,
  // timestamp } }`. Unwrap to the inner object (same helper the
  // viewers-changed / field-preview consumers use); a direct,
  // unwrapped frame passes through unchanged.
  const r = unwrapEventData(raw as Record<string, unknown>)
  if (!r || typeof r !== 'object') return null
  if (
    !Array.isArray(r.actions) ||
    typeof r.last_xid8 !== 'number' ||
    typeof r.last_sync_id !== 'number'
  ) {
    logger.warn('sync-actions SSE frame missing required fields', { frame: r })
    return null
  }
  return {
    actions: r.actions as SyncAction[],
    last_xid8: r.last_xid8 as number,
    last_sync_id: r.last_sync_id as number,
  }
}
