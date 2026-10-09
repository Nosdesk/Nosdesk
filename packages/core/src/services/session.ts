/**
 * The one place that refreshes the session and decides it is lost.
 *
 * Every caller that gets a 401 (the axios interceptors, the sync runtime's raw
 * fetches, the collab connection) refreshes through `refreshSession()`. What
 * a refresh result means for the app:
 * - `rejected`: the session is over. Local session state is torn down
 *   (`onSessionLost`, which leaves `hasSession()` false) and the host's
 *   handler runs (web: sign out and go to the login page), once per
 *   rejection: concurrent callers share one refresh.
 * - `unavailable`: the server couldn't be reached. Nobody is signed out; the
 *   caller backs off and tries later.
 *
 * The server decides whether the session is over: a refresh is always asked
 * of it, even with no session showing on the client (a CSRF cookie that
 * expired while the refresh cookie is still good just renews). Nothing about
 * the session is latched here, so any sign-in, however it happens, ends a
 * lost one without anyone having to say so.
 */
import { transport, type RefreshResult } from '../transport'

export type { RefreshResult }

let lostHandler: (() => void) | null = null
let inFlight: Promise<RefreshResult> | null = null

/** What the app does when its session is lost, besides `onSessionLost`.
 *  Registered once by the host at bootstrap. */
export function setSessionLostHandler(handler: (() => void) | null): void {
  lostHandler = handler
}

/** The client holds no session (a rejected refresh drops it). For what the UI
 *  says; whether to refresh is the server's call, not this. */
export function sessionGone(): boolean {
  return !transport().auth.hasSession()
}

/** Refresh the session, acting on a rejection once. Never throws. */
export function refreshSession(): Promise<RefreshResult> {
  inFlight ??= (async () => {
    try {
      const result = await transport().auth.refresh()
      if (result === 'rejected') {
        transport().auth.onSessionLost()
        lostHandler?.()
      }
      return result
    } finally {
      inFlight = null
    }
  })()
  return inFlight
}
