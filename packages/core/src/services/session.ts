/**
 * The one place that refreshes the session and decides it is lost.
 *
 * Every caller that gets a 401 (the axios interceptors, raw-fetch sync and
 * downloads, the collab connection) refreshes through `refreshSession()`. The
 * host's `AuthStrategy.refresh()` dedups concurrent refreshes; this adds what
 * a refresh result means for the app:
 * - `rejected`: the session is over. Local session state is torn down
 *   (`onSessionLost`) and the host's handler runs (web: sign out and go to
 *   the login page), once. Until a session is held again no caller posts
 *   another refresh; each gets `rejected` straight away.
 * - `unavailable`: the server couldn't be reached. Nobody is signed out; the
 *   caller backs off and tries later.
 */
import { transport, type RefreshResult } from '../transport'

export type { RefreshResult }

let lost = false
let lostHandler: (() => void) | null = null

/** What the app does when its session is lost, besides `onSessionLost`.
 *  Registered once by the host at bootstrap. */
export function setSessionLostHandler(handler: (() => void) | null): void {
  lostHandler = handler
}

/** A session was established (signed in, or confirmed by the server). */
export function sessionStarted(): void {
  lost = false
}

/** The session is gone: a refresh was rejected, or the client holds none. */
export function sessionGone(): boolean {
  return lost || !transport().auth.hasSession()
}

/** Refresh the session, acting on a rejection once. Never throws. */
export async function refreshSession(): Promise<RefreshResult> {
  if (lost && !transport().auth.hasSession()) return 'rejected'
  const result = await transport().auth.refresh()
  if (result === 'renewed') {
    lost = false
  } else if (result === 'rejected' && !lost) {
    lost = true
    transport().auth.onSessionLost()
    lostHandler?.()
  }
  return result
}
