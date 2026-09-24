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
