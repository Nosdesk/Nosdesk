import { describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// The mobile app signs in with a password (and a second factor, a passkey or a
// recovery code) through the same screens as the web. In bearer mode the server
// answers with the session's tokens in the body and sets no cookie, so the app
// has to take them from the response or every later request goes out
// unauthenticated.
const http = vi.hoisted(() => ({ fetch: vi.fn() }))
vi.mock('../../../mobile/node_modules/@tauri-apps/plugin-http/dist-js/index.js', () => ({
  fetch: http.fetch,
}))
vi.mock('../../../mobile/node_modules/@tauri-apps/api/core.js', () => ({
  invoke: vi.fn(async () => undefined),
}))
vi.mock('@/router', () => ({
  default: { push: vi.fn(), replace: vi.fn(), currentRoute: { value: { fullPath: '/', query: {} } } },
  landAfterLogin: vi.fn(async () => {}),
}))
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ resetToDefault: vi.fn(), loadThemeFromUser: vi.fn() }),
}))

describe('signing in on the mobile app with a password and a code', () => {
  it('holds the session the server issued', async () => {
    setActivePinia(createPinia())
    // As in the app: the auth store (and the web API module with it) loads
    // before the mobile bootstrap installs its own transport.
    const { useAuthStore } = await import('@/stores/auth')
    const { setSecureStore, configureServer } = await import('@nosdesk/mobile/transport')
    const { setupApiClient } = await import('@nosdesk/mobile/apiClient')
    const { transport } = await import('@nosdesk/core/transport')
    const save = vi.fn(async () => {})
    setSecureStore({ load: async () => null, save, clear: vi.fn(async () => {}) })
    await configureServer('https://help.example.com')
    setupApiClient()

    http.fetch.mockResolvedValue(
      new Response(
        JSON.stringify({
          success: true,
          csrf_token: 'csrf-1',
          user: { uuid: 'u-1', name: 'Agent' },
          access_token: 'access-1',
          refresh_token: 'refresh-1',
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )

    expect(await useAuthStore().verifyMfaAndLogin('agent@example.com', 'pw', '123456')).toBe(true)

    expect(transport().auth.authHeaders().Authorization).toBe('Bearer access-1')
    expect(transport().auth.hasSession()).toBe(true)
    expect(save).toHaveBeenCalledWith('refresh-1')
  })
})
