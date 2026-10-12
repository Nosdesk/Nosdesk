import { beforeEach, describe, expect, it, vi } from 'vitest'

// The store app is versioned apart from the server and meets servers of many
// versions. It refuses one it can't work with, at the connect screen and again
// at launch, and says which side needs updating.
const http = vi.hoisted(() => ({ fetch: vi.fn() }))
const tauri = vi.hoisted(() => ({ appVersion: '1.1.0' }))
vi.mock('../../../mobile/node_modules/@tauri-apps/plugin-http/dist-js/index.js', () => ({
  fetch: http.fetch,
}))
vi.mock('../../../mobile/node_modules/@tauri-apps/api/app.js', () => ({
  getVersion: vi.fn(async () => tauri.appVersion),
}))

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })

/** A server whose setup status answers, with `config` as its /api/config reply. */
function serverWith(config: Response) {
  http.fetch.mockImplementation(async (url: string) =>
    url.endsWith('/auth/setup/status') ? json({ requires_setup: false }) : config.clone(),
  )
}

const OLD_SERVER = /older version of Nosdesk\. The app needs Nosdesk 1\.1 or later/

describe('connecting the app to a server', () => {
  beforeEach(() => {
    http.fetch.mockReset()
    tauri.appVersion = '1.1.0'
  })

  it('refuses a 1.0 server, whose /api/config asks for sign-in', async () => {
    serverWith(json({ error: 'Authentication required', code: 'AUTH_REQUIRED' }, 401))
    const { validateServer } = await import('@nosdesk/mobile/serverConfig')
    const result = await validateServer('help.example.com')
    expect(result.ok).toBe(false)
    expect(result.error).toMatch(OLD_SERVER)
  })

  it('refuses a server with no /api/config', async () => {
    serverWith(new Response('', { status: 404 }))
    const { validateServer } = await import('@nosdesk/mobile/serverConfig')
    expect((await validateServer('help.example.com')).error).toMatch(OLD_SERVER)
  })

  it('accepts a 1.1 server, a release candidate included', async () => {
    const { validateServer } = await import('@nosdesk/mobile/serverConfig')
    for (const version of ['1.1.0', '1.1.0-rc.7', '1.2.3']) {
      serverWith(json({ server_version: version, min_app_version: '1.1.0' }))
      expect(await validateServer('help.example.com')).toEqual({
        ok: true,
        origin: 'https://help.example.com',
      })
    }
  })

  it('asks for an app update when the server needs a newer app', async () => {
    serverWith(json({ server_version: '1.3.0', min_app_version: '1.2.0' }))
    const { validateServer } = await import('@nosdesk/mobile/serverConfig')
    const result = await validateServer('help.example.com')
    expect(result.ok).toBe(false)
    expect(result.error).toMatch(/needs a newer version of the Nosdesk app/)
  })

  it('reports an unreachable server as unreachable, not as old', async () => {
    serverWith(new Response('', { status: 503 }))
    const { validateServer } = await import('@nosdesk/mobile/serverConfig')
    expect(await validateServer('help.example.com')).toEqual({
      ok: false,
      error: 'Could not reach that server',
    })
  })
})

describe('launching with a stored server', () => {
  beforeEach(() => {
    vi.resetModules()
    http.fetch.mockReset()
    Object.defineProperty(window, 'isTauri', { value: true, configurable: true })
  })

  async function launch(serverOrigin: string) {
    const { storage } = await import('@nosdesk/core/storage')
    storage().setItem('nosdesk:server-origin', serverOrigin)
    const gate = await import('./serverGate')
    await gate.initServerGate()
    await vi.waitFor(() => expect(http.fetch).toHaveBeenCalled())
    await new Promise((resolve) => setTimeout(resolve, 0))
    return gate
  }

  it('goes back to the connect screen, with the reason, when the server is too old', async () => {
    http.fetch.mockResolvedValue(json({ code: 'AUTH_REQUIRED' }, 401))
    const gate = await launch('https://help.example.com')
    await vi.waitFor(() => expect(gate.needsServerSelection.value).toBe(true))
    expect(gate.serverGateError.value).toMatch(OLD_SERVER)
  })

  it('stays put when the server is current or out of reach', async () => {
    http.fetch.mockResolvedValue(json({ server_version: '1.1.0', min_app_version: '1.1.0' }))
    let gate = await launch('https://help.example.com')
    expect(gate.needsServerSelection.value).toBe(false)

    vi.resetModules()
    http.fetch.mockReset()
    http.fetch.mockRejectedValue(new TypeError('offline'))
    gate = await launch('https://help.example.com')
    expect(gate.needsServerSelection.value).toBe(false)
    expect(gate.serverGateError.value).toBe('')
  })
})
