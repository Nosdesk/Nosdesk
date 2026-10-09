import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The live-update stream keeps trying to come back, however long the server
// is unreachable, and tries at once when the device is back online.
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAuthenticated: true }) }))
vi.mock('@nosdesk/core/apiClient', () => ({
  default: { post: async () => ({ data: { sse_token: 't', expires_in: 3600, refresh_buffer: 60 } }) },
}))
vi.mock('@nosdesk/core/transport', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/transport')>()),
  sseStreamUrl: (q: string) => `/api/events/stream?${q}`,
}))

class FakeEventSource {
  static opened: FakeEventSource[] = []
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  constructor(public url: string) {
    FakeEventSource.opened.push(this)
  }
  addEventListener() {}
  close() {}
}

import { useSSE } from '../sseService'

const sse = useSSE()

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('EventSource', FakeEventSource)
  FakeEventSource.opened = []
})
afterEach(() => {
  sse.stop()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

/** The newest stream fails; let its retry run. */
async function failAndWait(ms: number) {
  FakeEventSource.opened.at(-1)!.onerror!()
  await vi.advanceTimersByTimeAsync(ms)
}

describe('the live-update stream', () => {
  it('keeps reconnecting after many failures, at most 30 seconds apart', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened).toHaveLength(1)

    for (let i = 0; i < 15; i++) await failAndWait(30_000)

    // One stream per attempt: the first plus a retry after each failure.
    expect(FakeEventSource.opened).toHaveLength(16)
  })

  it('reconnects at once when the device comes back online', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    for (let i = 0; i < 6; i++) await failAndWait(30_000)
    const before = FakeEventSource.opened.length
    FakeEventSource.opened.at(-1)!.onerror!()
    // The next retry is up to 30 seconds off; coming back online doesn't wait.
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened.length).toBe(before + 1)
  })
})
