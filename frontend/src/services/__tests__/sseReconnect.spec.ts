import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The live-update stream keeps trying to come back, however long the server
// is unreachable, and tries at once when the device is back online.
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAuthenticated: true }) }))
const api = vi.hoisted(() => ({
  hangNext: 0,
  /** Hold the next token request until the test answers it. */
  holdNext: false,
  held: null as (() => void) | null,
  configs: [] as Array<{ timeout?: number } | undefined>,
  minted: 0,
}))
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post: async (_url: string, _body?: unknown, config?: { timeout?: number }) => {
      api.configs.push(config)
      if (api.hangNext > 0) {
        api.hangNext--
        return new Promise(() => {})
      }
      // Each token is a new one, so a stream's URL shows which it was given.
      const data = { sse_token: `t${++api.minted}`, expires_in: 3600, refresh_buffer: 60 }
      if (api.holdNext) {
        api.holdNext = false
        return new Promise((resolve) => {
          api.held = () => resolve({ data })
        })
      }
      return { data }
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
import { setActiveWorkspaceSlug } from '@/services/activeWorkspace'

const sse = useSSE()

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('EventSource', FakeEventSource)
  FakeEventSource.opened = []
  api.hangNext = 0
  api.holdNext = false
  api.held = null
  api.configs.length = 0
  api.minted = 0
})
afterEach(() => {
  sse.stop()
  setActiveWorkspaceSlug(null)
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

/** The newest stream fails; let its retry run. */
async function failAndWait(ms: number) {
  FakeEventSource.opened.at(-1)!.onerror!()
  await vi.advanceTimersByTimeAsync(ms)
}

describe('the live-update stream', () => {
  it('subscribes once, to the caller and global topics; ticket presence is not on it', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    const url = new URL(FakeEventSource.opened[0].url, 'http://test')
    expect(url.searchParams.get('topics')).toBe('user,global')
  })

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

  it('retries within 8 s after a long outage while the browser says it is online', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    for (let i = 0; i < 6; i++) await failAndWait(30_000)
    const before = FakeEventSource.opened.length
    await failAndWait(8_000)
    expect(FakeEventSource.opened.length).toBe(before + 1)
  })

  it('still waits up to 30 s between retries while the browser says it is offline', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    window.dispatchEvent(new Event('offline'))
    Object.defineProperty(navigator, 'onLine', { value: false, configurable: true })
    try {
      for (let i = 0; i < 6; i++) await failAndWait(30_000)
      const before = FakeEventSource.opened.length
      await failAndWait(8_000)
      expect(FakeEventSource.opened.length).toBe(before)
    } finally {
      Object.defineProperty(navigator, 'onLine', { value: true, configurable: true })
      window.dispatchEvent(new Event('online'))
    }
  })
})

/** The token the stream at `index` was opened with. */
function tokenOf(index: number): string | null {
  return new URL(FakeEventSource.opened[index].url, 'http://localhost').searchParams.get('sse_token')
}

// The server binds a stream token to the workspace selected when it was asked
// for, and refuses the stream under any other. EventSource can't say a stream
// was refused, so a failed stream never retries with the token it was given.
describe('the stream token', () => {
  it('is asked for again after the stream fails, rather than retried until it expires', async () => {
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    expect(tokenOf(0)).toBe('t1')

    await failAndWait(8_000)
    expect(tokenOf(1)).toBe('t2')
    await failAndWait(8_000)
    expect(tokenOf(2)).toBe('t3')
  })

  it('is reused while the workspace stays the same', async () => {
    setActiveWorkspaceSlug('acme')
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    FakeEventSource.opened[0].onopen!()
    // Back online after an outage, the stream starts over.
    window.dispatchEvent(new Event('offline'))
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened).toHaveLength(2)
    expect(tokenOf(1)).toBe('t1')
  })

  it('from one workspace is not used in another', async () => {
    setActiveWorkspaceSlug('acme')
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    FakeEventSource.opened[0].onopen!()

    setActiveWorkspaceSlug('beta')
    window.dispatchEvent(new Event('offline'))
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(0)
    expect(tokenOf(1)).toBe('t2')
  })

  it('answered after the stream was closed is not used for the next workspace', async () => {
    setActiveWorkspaceSlug('acme')
    api.holdNext = true
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    // Leaving the workspace closes the stream while its token is on the way.
    sse.stop()
    api.held!()
    await vi.advanceTimersByTimeAsync(0)

    setActiveWorkspaceSlug('beta')
    sse.start()
    await vi.advanceTimersByTimeAsync(0)
    expect(FakeEventSource.opened).toHaveLength(1)
    expect(tokenOf(0)).toBe('t2')
  })
})
