import { afterEach, describe, expect, it, vi } from 'vitest'

// A bearer client (the mobile app) sends its access token in a header. When
// it expires, sync refreshes it and retries: the retry has to send the new
// token, or it gets the same 401 again.
const auth = vi.hoisted(() => ({ token: 'expired' }))
vi.mock('@nosdesk/core/transport', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/transport')>()),
  apiBaseUrl: () => 'https://help.example.com/api',
  transport: () => ({
    auth: {
      useCredentials: false,
      authHeaders: () => ({ Authorization: `Bearer ${auth.token}` }),
      refresh: async () => {
        auth.token = 'fresh'
        return 'renewed'
      },
      hasSession: () => true,
      onSessionLost: () => {},
    },
  }),
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))

const { fetchServerIdentity } = await import('./lifecycle')

afterEach(() => vi.unstubAllGlobals())

describe('a sync request after the access token expired', () => {
  it('retries with the refreshed token', async () => {
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ server_schema: 'abc', instance_id: 'i-1' }), { status: 200 }),
      )
    vi.stubGlobal('fetch', fetch)

    expect(await fetchServerIdentity()).toEqual({ schemaHash: 'abc', instanceId: 'i-1' })
    expect(fetch).toHaveBeenCalledTimes(2)
    const sent = (call: number) => (fetch.mock.calls[call][1] as RequestInit).headers as Record<string, string>
    expect(sent(0).Authorization).toBe('Bearer expired')
    expect(sent(1).Authorization).toBe('Bearer fresh')
  })
})
