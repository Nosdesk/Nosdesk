import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// A ticket edit pushed after the access token expired: the push refreshes the
// session and retries, as every other sync request does, instead of backing
// off until some other request happens to refresh it.

const auth = vi.hoisted(() => ({
  token: 'expired',
  refreshes: 0,
  /** What the server says to a refresh. */
  answer: 'renewed' as 'renewed' | 'rejected' | 'unavailable',
  held: true,
  lost: 0,
}))
vi.mock('@nosdesk/core/transport', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/transport')>()),
  apiBaseUrl: () => 'https://help.example.com/api',
  transport: () => ({
    auth: {
      useCredentials: false,
      authHeaders: () => ({ Authorization: `Bearer ${auth.token}` }),
      refresh: async () => {
        auth.refreshes++
        if (auth.answer === 'renewed') auth.token = 'fresh'
        return auth.answer
      },
      hasSession: () => auth.held,
      onSessionLost: () => {
        auth.lost++
        auth.held = false
      },
    },
  }),
}))
vi.mock('@/services/activeWorkspace', () => ({ workspaceHeaders: () => ({}) }))
vi.mock('@nosdesk/core/sync/pool', () => ({
  currentEpoch: () => 1,
  patch: vi.fn(),
  get: vi.fn(),
  getLastSyncId: () => null,
}))

const pending = vi.hoisted(() => ({ txs: [] as Array<{ tx_id: string }> }))
vi.mock('./idb', () => ({
  loadTransactions: async () => [...pending.txs],
  deleteTransaction: async (_h: unknown, txId: string) => {
    pending.txs = pending.txs.filter((t) => t.tx_id !== txId)
  },
  putTransaction: vi.fn(),
}))

const TX = {
  tx_id: 'tx-1',
  aggregate: 'ticket',
  model_id: '7',
  op: 'U',
  patch: { title: 'New title' },
  base_sync_id: null,
  createdAt: 0,
}

let queue: typeof import('./queue')
beforeEach(async () => {
  vi.useFakeTimers()
  vi.resetModules()
  queue = await import('./queue')
  queue.setIdbHandle({} as never)
  pending.txs = [TX]
  auth.token = 'expired'
  auth.refreshes = 0
  auth.answer = 'renewed'
  auth.held = true
  auth.lost = 0
})
afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

const accepted = () =>
  new Response(JSON.stringify({ applied: ['tx-1'], rejected: [] }), { status: 200 })
const sentToken = (fetch: ReturnType<typeof vi.fn>, call: number) =>
  ((fetch.mock.calls[call][1] as RequestInit).headers as Record<string, string>).Authorization

describe('a sync push after the access token expired', () => {
  it('refreshes once and retries with the new token', async () => {
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(accepted())
    vi.stubGlobal('fetch', fetch)

    await queue.flush()

    expect(auth.refreshes).toBe(1)
    expect(fetch).toHaveBeenCalledTimes(2)
    expect(sentToken(fetch, 0)).toBe('Bearer expired')
    expect(sentToken(fetch, 1)).toBe('Bearer fresh')
    expect((fetch.mock.calls[1][1] as RequestInit).body).toBe(
      (fetch.mock.calls[0][1] as RequestInit).body,
    )
    expect(pending.txs).toEqual([])
  })

  it('backs off when the session was rejected, without a loop', async () => {
    auth.answer = 'rejected'
    const fetch = vi.fn(async () => new Response(null, { status: 401 }))
    vi.stubGlobal('fetch', fetch)

    await queue.flush()
    expect(auth.refreshes).toBe(1)
    expect(auth.lost).toBe(1)
    expect(fetch).toHaveBeenCalledTimes(1)

    // Nothing more until the backoff runs out, then one more push. (Sign-out
    // tears the sync runtime down, which ends the retries.)
    await vi.advanceTimersByTimeAsync(499)
    expect(fetch).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(1)
    expect(fetch).toHaveBeenCalledTimes(2)
    expect(pending.txs).toEqual([TX])
  })

  it('stops pushing once the sync runtime is torn down', async () => {
    auth.answer = 'rejected'
    const fetch = vi.fn(async () => new Response(null, { status: 401 }))
    vi.stubGlobal('fetch', fetch)

    await queue.flush()
    queue.setIdbHandle(null)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(fetch).toHaveBeenCalledTimes(1)
    expect(auth.refreshes).toBe(1)
  })

  it('backs off without signing out when the server cannot be reached to refresh', async () => {
    auth.answer = 'unavailable'
    const fetch = vi.fn(async () => new Response(null, { status: 401 }))
    vi.stubGlobal('fetch', fetch)

    await queue.flush()
    expect(auth.refreshes).toBe(1)
    expect(fetch).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(500)
    expect(fetch).toHaveBeenCalledTimes(2)
    expect(auth.refreshes).toBe(2)
    expect(auth.lost).toBe(0)
    expect(pending.txs).toEqual([TX])
  })
})
