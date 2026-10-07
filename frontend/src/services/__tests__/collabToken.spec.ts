import { beforeEach, describe, expect, it, vi } from 'vitest'

let calls = 0
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post: vi.fn(async () => {
      calls++
      return { data: { token: `t${calls}`, expires_in: 120, refresh_buffer: 30 } }
    }),
  },
}))

import * as collabToken from '@/services/collabToken'
import { getCollabToken, peekCollabToken, resetCollabToken } from '@/services/collabToken'

beforeEach(() => {
  calls = 0
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
    collabToken.keepCollabTokenWarm()
    await vi.advanceTimersByTimeAsync(0)
    expect(peekCollabToken()).toBe('t1')

    await vi.advanceTimersByTimeAsync(91_000)
    expect(peekCollabToken()).toBe('t2')
    expect(calls).toBe(2)
  })

  it('stops when the session or workspace ends', async () => {
    vi.useFakeTimers()
    collabToken.keepCollabTokenWarm()
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
