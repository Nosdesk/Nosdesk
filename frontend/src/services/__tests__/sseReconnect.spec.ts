import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The live-update stream keeps trying to come back, however long the server
// is unreachable, and tries at once when the device is back online.
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAuthenticated: true }) }))
const api = vi.hoisted(() => ({ hangNext: 0, configs: [] as Array<{ timeout?: number } | undefined> }))
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post: async (_url: string, _body?: unknown, config?: { timeout?: number }) => {
      api.configs.push(config)
      if (api.hangNext > 0) {
        api.hangNext--
        return new Promise(() => {})
      }
      return { data: { sse_token: 't', expires_in: 3600, refresh_buffer: 60 } }
    },
  },
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
  closed = false
  close() {
    this.closed = true
  }
}

import { useSSE } from '../sseService'

const sse = useSSE()

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('EventSource', FakeEventSource)
  FakeEventSource.opened = []
  api.hangNext = 0
  api.configs.length = 0
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

  it('reopens a stream that went through an outage, even one that still looks open', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    FakeEventSource.opened[0].onopen!()
    window.dispatchEvent(new Event('offline'))
    await vi.advanceTimersByTimeAsync(20_000)

    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened).toHaveLength(2)
    expect(FakeEventSource.opened[0].closed).toBe(true)
  })

  it('leaves an open stream alone when the tab is shown again', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    FakeEventSource.opened[0].onopen!()
    document.dispatchEvent(new Event('visibilitychange'))
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened).toHaveLength(1)
  })

  it("doesn't wait on a token fetch from before the network came back", async () => {
    api.hangNext = 1
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    window.dispatchEvent(new Event('offline'))
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened).toHaveLength(1)
  })

  it('gives up on a token fetch that never answers', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    expect(api.configs[0]?.timeout).toBeGreaterThan(0)
  })
})
