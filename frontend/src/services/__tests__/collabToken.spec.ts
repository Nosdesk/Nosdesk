import { beforeEach, describe, expect, it, vi } from 'vitest'

let calls = 0
/** Calls that fail, as when the API is briefly unreachable. */
let failNext = 0
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post: vi.fn(async () => {
      calls++
      if (failNext > 0) {
        failNext--
        throw new Error('network down')
      }
      return { data: { token: `t${calls}`, expires_in: 120, refresh_buffer: 30 } }
    }),
  },
}))
const routing = vi.hoisted(() => ({ mode: 'host' as 'host' | 'path' }))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({ getWorkspaceRouting: () => routing.mode }))

import * as collabToken from '@/services/collabToken'

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true })
  document.dispatchEvent(new Event('visibilitychange'))
}
import { getCollabToken, peekCollabToken, resetCollabToken } from '@/services/collabToken'

beforeEach(() => {
  calls = 0
  failNext = 0
  routing.mode = 'host'
  setVisibility('visible')
  resetCollabToken()
  vi.useRealTimers()
})

describe('collab token', () => {
  it('shares one fetch between concurrent callers', async () => {
    const [a, b] = await Promise.all([getCollabToken(), getCollabToken()])
    expect(a).toBe('t1')
    expect(b).toBe('t1')
    expect(calls).toBe(1)
  })

  it('peeks a token only while it is still good to connect with', async () => {
    vi.useFakeTimers()
    expect(peekCollabToken()).toBeNull()
    await getCollabToken()
    expect(peekCollabToken()).toBe('t1')
    // 120s TTL less the 30s buffer.
    vi.advanceTimersByTime(91_000)
    expect(peekCollabToken()).toBeNull()
  })
})

// Opening a note shouldn't wait on a token round trip, so one is kept ready
// once the app has loaded, while the page is visible.
describe('a token kept ready', () => {
  it('is fetched at once and again as each one runs out', async () => {
    vi.useFakeTimers()
    collabToken.keepCollabTokenWarm('acme')
    await vi.advanceTimersByTimeAsync(0)
    expect(peekCollabToken()).toBe('t1')

    await vi.advanceTimersByTimeAsync(91_000)
    expect(peekCollabToken()).toBe('t2')
    expect(calls).toBe(2)
  })

  it('stops when the session or workspace ends', async () => {
    vi.useFakeTimers()
    collabToken.keepCollabTokenWarm('acme')
    await vi.advanceTimersByTimeAsync(0)
    resetCollabToken()

    await vi.advanceTimersByTimeAsync(200_000)
    expect(calls).toBe(1)
    expect(peekCollabToken()).toBeNull()
  })

  it('is not cached when the workspace changed while it was on the way', async () => {
    const fetching = getCollabToken()
    resetCollabToken()
    await fetching
    expect(peekCollabToken()).toBeNull()
  })
})

describe('a token kept ready, when fetching fails', () => {
  it('tries again later instead of giving up', async () => {
    vi.useFakeTimers()
    failNext = 1
    collabToken.keepCollabTokenWarm('acme')
    await vi.advanceTimersByTimeAsync(0)
    expect(peekCollabToken()).toBeNull()

    await vi.advanceTimersByTimeAsync(5_000)
    expect(calls).toBe(2)
    expect(peekCollabToken()).toBe('t2')
  })

  it('tries again at once when the page is shown again', async () => {
    vi.useFakeTimers()
    failNext = 1
    collabToken.keepCollabTokenWarm('acme')
    await vi.advanceTimersByTimeAsync(0)
    setVisibility('hidden')
    await vi.advanceTimersByTimeAsync(60_000)
    // Hidden: the retry waits.
    expect(calls).toBe(1)

    setVisibility('visible')
    await vi.advanceTimersByTimeAsync(0)
    expect(calls).toBe(2)
    expect(peekCollabToken()).toBe('t2')
  })
})

describe('a token kept ready in path routing', () => {
  it('waits for a workspace to be chosen', async () => {
    vi.useFakeTimers()
    routing.mode = 'path'
    collabToken.keepCollabTokenWarm(null)
    await vi.advanceTimersByTimeAsync(0)
    expect(calls).toBe(0)

    collabToken.keepCollabTokenWarm('acme')
    await vi.advanceTimersByTimeAsync(0)
    expect(calls).toBe(1)
  })
})
