/**
 * Retry a request the server answered with 429, after the wait it asked for.
 *
 * A 429 says nothing about the session: the person is still signed in, the
 * server just wants them to slow down. A check that decides signed-in vs
 * signed-out (the profile fetch behind the route guard) waits it out instead
 * of treating it as a failure, so a busy shared office IP never lands someone
 * on the login page. Any other error goes straight back to the caller.
 */

/** The longest a caller waits in total before giving the 429 back. The rate
 *  limiter's window is a minute, so a full window always fits. */
export const RATE_LIMIT_MAX_WAIT_MS = 60_000

type RateLimitedError = {
  response?: {
    status?: number
    headers?: Record<string, unknown>
    data?: { retry_after?: unknown }
  }
}

function isRateLimited(err: unknown): boolean {
  return (err as RateLimitedError)?.response?.status === 429
}

/** The wait the server asked for, in ms: the body's `retry_after` or the
 *  `Retry-After` header (seconds), and one second when it gave neither. */
export function retryAfterMs(err: unknown): number {
  const response = (err as RateLimitedError)?.response
  const candidates = [response?.data?.retry_after, response?.headers?.['retry-after']]
  for (const value of candidates) {
    const seconds = typeof value === 'string' ? Number(value) : value
    if (typeof seconds === 'number' && Number.isFinite(seconds) && seconds > 0) {
      return seconds * 1000
    }
  }
  return 1000
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

export async function withRateLimitRetry<T>(
  request: () => Promise<T>,
  opts: { maxWaitMs?: number; wait?: (ms: number) => Promise<void> } = {},
): Promise<T> {
  const maxWaitMs = opts.maxWaitMs ?? RATE_LIMIT_MAX_WAIT_MS
  const wait = opts.wait ?? sleep
  let waited = 0
  for (;;) {
    try {
      return await request()
    } catch (err) {
      if (!isRateLimited(err) || waited >= maxWaitMs) throw err
      const ms = Math.min(retryAfterMs(err), maxWaitMs - waited)
      waited += ms
      await wait(ms)
    }
  }
}
