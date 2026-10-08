import { describe, expect, it, vi } from 'vitest'

// The mobile app's bearer session. The server rotates the refresh token on
// every refresh and treats a reused one as stolen, signing the device out.
// A sync request and an API request can both get a 401 when the access
// token expires: they have to share one refresh.
const http = vi.hoisted(() => ({ fetch: vi.fn() }))
// The Tauri modules are the mobile package's dependencies, not this app's, so
// they are mocked by the file the mobile code resolves them to.
vi.mock('../../../mobile/node_modules/@tauri-apps/plugin-http/dist-js/index.js', () => ({
  fetch: http.fetch,
}))
vi.mock('../../../mobile/node_modules/@tauri-apps/api/core.js', () => ({
  invoke: vi.fn(async () => undefined),
}))

import { configureServer, setSecureStore } from '@nosdesk/mobile/transport'
import { transport } from '@nosdesk/core/transport'

function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

describe('two requests refused at once with an expired token', () => {
  it('refresh the session once, and both get the new token', async () => {
    setSecureStore({ load: async () => 'refresh-1', save: vi.fn(async () => {}), clear: vi.fn(async () => {}) })
    await configureServer('https://help.example.com')

    const answer = deferred<Response>()
    http.fetch.mockReturnValueOnce(answer.promise)
    const sync = transport().auth.refresh()
    const api = transport().auth.refresh()
    answer.resolve(
      new Response(JSON.stringify({ access_token: 'access-2', refresh_token: 'refresh-2' }), { status: 200 }),
    )

    const results = await Promise.all([sync, api])
    expect(http.fetch).toHaveBeenCalledTimes(1)
    expect(results).toEqual([true, true])
    expect(http.fetch).toHaveBeenCalledTimes(1)
    expect(transport().auth.authHeaders().Authorization).toBe('Bearer access-2')
  })
})
