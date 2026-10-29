import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The session refreshes shortly before its access token expires, instead of
// after a request comes back 401. One refresh renews every tab.

vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ sessionLost: vi.fn() }) }))

const CSRF_EXPIRED = 'expires=Thu, 01 Jan 1970 00:00:00 GMT'
/** Fifteen minutes, as the server issues them. */
const TTL = 900
/** When the schedule refreshes: two minutes before expiry. */
const DUE_MS = (TTL - 120) * 1000

/** The refresh POST, shared by every tab; it answers with `headers`. */
const post = vi.fn()
const stops: (() => void)[] = []

/** One tab: the web platform booted, its refresh answered by `post`. */
async function bootTab() {
  const axios = (await import('axios')).default
  vi.spyOn(axios, 'post').mockImplementation(post as never)
  const { configurePlatform } = await import('@/platform')
  await configurePlatform()
  const schedule = await import('@nosdesk/core/services/sessionSchedule')
  stops.push(schedule.stopSessionSchedule)
  return schedule
}

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true })
}

/** Web Locks as the browser grants them: one holder at a time, in order. */
function stubLocks() {
  let chain: Promise<unknown> = Promise.resolve()
  Object.defineProperty(navigator, 'locks', {
    configurable: true,
    value: {
      request: (_name: string, task: () => Promise<unknown>) => {
        const run = chain.then(task)
        chain = run.catch(() => undefined)
        return run
      },
    },
  })
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.resetModules()
  localStorage.clear()
  post.mockReset()
  post.mockImplementation(async () => ({ status: 200, data: {}, headers: { 'x-access-expires-in': String(TTL) } }))
  setVisibility('visible')
  // Signed in: the server set the JS-readable CSRF cookie.
  document.cookie = 'csrf_token=first; path=/'
})
afterEach(() => {
  stops.splice(0).forEach((stop) => stop())
  vi.restoreAllMocks()
  vi.useRealTimers()
  Reflect.deleteProperty(navigator, 'locks')
  document.cookie = `csrf_token=; path=/; ${CSRF_EXPIRED}`
})

describe('refreshing before the access token expires', () => {
  it('refreshes two minutes before expiry, then again two minutes before the next', async () => {
    const { noteAccessExpiry } = await bootTab()
    noteAccessExpiry(String(TTL))

    await vi.advanceTimersByTimeAsync(DUE_MS - 1000)
    expect(post).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1000)
    expect(post).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(DUE_MS - 1000)
    expect(post).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1000)
    expect(post).toHaveBeenCalledTimes(2)
  })

  it('keeps the schedule in a hidden tab', async () => {
    const { noteAccessExpiry } = await bootTab()
    setVisibility('hidden')
    noteAccessExpiry(String(TTL))

    await vi.advanceTimersByTimeAsync(DUE_MS)

    expect(post).toHaveBeenCalledTimes(1)
  })

  it('refreshes at once when the tab comes back after the deadline', async () => {
    const { noteAccessExpiry } = await bootTab()
    noteAccessExpiry(String(TTL))
    setVisibility('hidden')

    // The background tab's timer didn't run while the clock moved on.
    vi.setSystemTime(Date.now() + (TTL + 60) * 1000)
    setVisibility('visible')
    document.dispatchEvent(new Event('visibilitychange'))
    await vi.advanceTimersByTimeAsync(0)

    expect(post).toHaveBeenCalledTimes(1)
  })

  it('refreshes once across two tabs', async () => {
    stubLocks()
    const tabA = await bootTab()
    vi.resetModules()
    const tabB = await bootTab()
    tabA.noteAccessExpiry(String(TTL))
    tabB.noteAccessExpiry(String(TTL))

    await vi.advanceTimersByTimeAsync(DUE_MS)

    expect(post).toHaveBeenCalledTimes(1)
  })

  it('keeps the renewed expiry when an older answer arrives after it', async () => {
    const { noteAccessExpiry } = await bootTab()
    noteAccessExpiry(String(TTL))
    await vi.advanceTimersByTimeAsync(DUE_MS)
    expect(post).toHaveBeenCalledTimes(1)

    // A delta sent with the previous token comes back after the renewal.
    noteAccessExpiry('100')
    await vi.advanceTimersByTimeAsync(100_000)

    expect(post).toHaveBeenCalledTimes(1)
  })

  it('stops, rather than refreshing again, when a renewal does not say for how long', async () => {
    post.mockImplementation(async () => ({ status: 200, data: {}, headers: {} }))
    const { noteAccessExpiry } = await bootTab()
    noteAccessExpiry(String(TTL))

    await vi.advanceTimersByTimeAsync(DUE_MS + 10 * 60_000)

    expect(post).toHaveBeenCalledTimes(1)
  })

  it('does not refresh a session that is gone', async () => {
    const { noteAccessExpiry } = await bootTab()
    noteAccessExpiry(String(TTL))
    document.cookie = `csrf_token=; path=/; ${CSRF_EXPIRED}`

    await vi.advanceTimersByTimeAsync(DUE_MS)

    expect(post).not.toHaveBeenCalled()
  })

  it('ignores a response without the header', async () => {
    const { noteAccessExpiry } = await bootTab()
    noteAccessExpiry(undefined)
    noteAccessExpiry('')
    noteAccessExpiry('soon')

    await vi.advanceTimersByTimeAsync(60 * 60_000)

    expect(post).not.toHaveBeenCalled()
  })
})
