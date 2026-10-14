import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// The mobile app loses its session the same way the web does: the server
// rejects the refresh (a revoked session, or every session after an upgrade
// that revokes them). It has to tear the session's state down too, or the next
// sign-in reuses the old sync runtime and stores and loads nothing.
const http = vi.hoisted(() => ({ fetch: vi.fn() }))
vi.mock('../../../mobile/node_modules/@tauri-apps/plugin-http/dist-js/index.js', () => ({
  fetch: http.fetch,
}))
vi.mock('../../../mobile/node_modules/@tauri-apps/api/core.js', () => ({
  invoke: vi.fn(async () => undefined),
}))
// The shell's own bootstrap (server choice, storage, plugins) is beside the
// point here; it ends by pointing the transport at the server.
vi.mock('@nosdesk/mobile', async () => {
  const { configureServer, setSecureStore } = await import('@nosdesk/mobile/transport')
  return {
    tauriSecureStore: () => ({}),
    bootstrapMobile: async () => {
      setSecureStore({ load: async () => 'refresh-1', save: vi.fn(async () => {}), clear: vi.fn(async () => {}) })
      await configureServer('https://help.example.com')
    },
  }
})

const router = vi.hoisted(() => ({
  push: vi.fn(),
  replace: vi.fn(),
  currentRoute: { value: { fullPath: '/tickets', name: 'tickets', query: {} } },
}))
vi.mock('@/router', () => ({ default: router, landAfterLogin: vi.fn() }))
const reset = vi.hoisted(() => ({ resetWorkspaceScopedState: vi.fn(async () => {}) }))
vi.mock('@/stores/workspaceReset', () => reset)
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ resetToDefault: vi.fn(), loadThemeFromUser: vi.fn() }),
}))

beforeEach(() => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  ;(globalThis as { isTauri?: boolean }).isTauri = true
  window.history.replaceState({}, '', '/tickets')
})
afterEach(() => {
  delete (globalThis as { isTauri?: boolean }).isTauri
  vi.useRealTimers()
})

describe('the mobile app, when the server rejects its refresh', () => {
  it('clears the session state and goes to sign-in', async () => {
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { useAuthStore } = await import('@/stores/auth')
    const auth = useAuthStore()
    auth.user = { uuid: 'u-1' } as never
    http.fetch.mockResolvedValue(new Response('{}', { status: 401 }))

    const { refreshSession } = await import('@nosdesk/core/services/session')
    expect(await refreshSession()).toBe('rejected')
    await vi.advanceTimersByTimeAsync(1000)

    expect(reset.resetWorkspaceScopedState).toHaveBeenCalledWith('sign-out')
    expect(auth.user).toBeNull()
    expect(router.push).toHaveBeenCalled()
  })
})
