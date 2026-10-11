/**
 * The wait before a reconnect's next try, shared by the note token retry and
 * the live-update stream.
 *
 * While the browser says it is offline its `online` event says when to try
 * again, so the wait can grow to 30 s. While it says online, a dropped link
 * can come back with no event at all and only a retry finds out, so the wait
 * tops out at 5 to 8 s, spread so clients that lost a server together (a
 * deploy) don't all come back to it at once.
 */

const OFFLINE_MAX_MS = 30_000
const ONLINE_MAX_MIN_MS = 5_000
const ONLINE_MAX_MAX_MS = 8_000

/** `firstMs` doubling with each of `failures`, capped as above. */
export function retryDelay(failures: number, firstMs: number, offline: boolean): number {
  const max = offline
    ? OFFLINE_MAX_MS
    : ONLINE_MAX_MIN_MS + Math.random() * (ONLINE_MAX_MAX_MS - ONLINE_MAX_MIN_MS)
  return Math.min(max, firstMs * 2 ** failures)
}
