/**
 * Refreshes the session shortly before its access token expires, so requests
 * don't go out without one and come back 401.
 *
 * The server says how many seconds the access token has left on every
 * signed-in response and on a refresh (`X-Access-Expires-In`); the host's
 * response handlers pass it to `noteAccessExpiry`. The refresh itself is
 * `refreshSession()`, shared with the 401 path, which stays as the fallback.
 *
 * Tabs share the session's cookies, so one refresh renews them all. Through
 * the host (`configureSessionSchedule`), the refresh runs under a cross-tab
 * lock and the expiry is shared: a tab that waited for the lock reads the
 * expiry another tab just renewed and leaves it, rather than rotating the
 * token again. Hidden tabs keep their schedule (a forgotten tab still polls)
 * under the same lock. Background tabs' timers run late, so the host also
 * wakes the schedule when the page becomes visible or comes back online.
 */
import { refreshSession, sessionGone } from './session'

/** The response header the server sends the seconds left in. */
export const ACCESS_EXPIRES_IN_HEADER = 'x-access-expires-in'

/** What the schedule needs from the browser. Without a host, each tab keeps
 *  its own schedule and only its timer wakes it. */
export interface SessionScheduleHost {
  /** Run `task` holding the one cross-tab refresh lock. */
  withLock(task: () => Promise<void>): Promise<void>
  /** The expiry (epoch ms) recorded by any tab, or null. */
  readShared(): number | null
  writeShared(at: number): void
  /** Call `wake` when the page becomes visible or comes back online, and
   *  `sharedChanged` when another tab records an expiry. Returns the
   *  unsubscribe. */
  listen(handlers: { wake(): void; sharedChanged(): void }): () => void
}

/** How long before expiry to refresh. Background tabs' timers can run a
 *  minute late. */
const LEAD_MS = 2 * 60_000
/** After a refresh that couldn't reach the server, try again this much later. */
const RETRY_MS = 30_000

let host: SessionScheduleHost | null = null
let unlisten: (() => void) | null = null
let expiresAt: number | null = null
let timer: ReturnType<typeof setTimeout> | null = null

/** Install the browser side. Registered once by the host at bootstrap. */
export function configureSessionSchedule(next: SessionScheduleHost | null): void {
  unlisten?.()
  host = next
  unlisten =
    next?.listen({
      wake: () => void refreshIfDue(),
      sharedChanged: () => {
        adoptShared()
        arm()
      },
    }) ?? null
}

/** Record how long the access token has left: the header's value, in seconds.
 *  Anything else (no header, an older server) is ignored. */
export function noteAccessExpiry(secondsLeft: string | number | null | undefined): void {
  if (secondsLeft === null || secondsLeft === undefined || secondsLeft === '') return
  const seconds = Number(secondsLeft)
  if (!Number.isFinite(seconds) || seconds < 0) return
  const at = Date.now() + seconds * 1000
  // The session's expiry only moves forward. An answer to a request sent with
  // the previous token can arrive after a renewal; taken as is, it would make
  // the next tab to look think the session is due and rotate it again.
  adoptShared()
  if (expiresAt === null || at > expiresAt) {
    expiresAt = at
    host?.writeShared(at)
  }
  arm()
}

function arm(delay?: number): void {
  if (timer !== null) clearTimeout(timer)
  timer = null
  if (expiresAt === null) return
  const wait = delay ?? Math.max(0, expiresAt - LEAD_MS - Date.now())
  timer = setTimeout(() => void refreshIfDue(), wait)
}

function due(): boolean {
  return expiresAt !== null && Date.now() >= expiresAt - LEAD_MS
}

/** Take a later expiry another tab recorded. */
function adoptShared(): void {
  const shared = host?.readShared() ?? null
  if (shared !== null && (expiresAt === null || shared > expiresAt)) expiresAt = shared
}

async function refreshIfDue(): Promise<void> {
  adoptShared()
  if (expiresAt === null || sessionGone()) return
  if (!due()) {
    arm()
    return
  }
  const run = async () => {
    // Another tab may have renewed while this one waited for the lock.
    adoptShared()
    if (!due()) {
      arm()
      return
    }
    const before = expiresAt
    const result = await refreshSession()
    if (result === 'unavailable') {
      arm(RETRY_MS)
    } else if (result === 'renewed' && expiresAt === before) {
      // Renewed, but the answer didn't say for how long: stop until a
      // response does, rather than refreshing again straight away.
      expiresAt = null
      arm()
    }
    // A rejected refresh has ended the session; a sign-in starts it again.
  }
  await (host ? host.withLock(run) : run())
}

/** Forget the schedule and its host. For tests. */
export function stopSessionSchedule(): void {
  configureSessionSchedule(null)
  if (timer !== null) clearTimeout(timer)
  timer = null
  expiresAt = null
}
