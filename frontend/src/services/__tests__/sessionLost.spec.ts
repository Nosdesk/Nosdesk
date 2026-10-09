import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

// A lost session is not a sign-out. When the server rejects the refresh the
// session is already over, so the tab only tears down locally. It must not ask
// the server to sign out: the cookie jar is shared by every tab, so a late
// /auth/logout from a background tab would end the session another tab has
// just signed in with.

const router = vi.hoisted(() => ({
  push: vi.fn(),
  replace: vi.fn(),
  currentRoute: { value: { fullPath: '/tickets', name: 'tickets', query: {} } },
}))
vi.mock('@/router', () => ({ default: router, landAfterLogin: vi.fn() }))
vi.mock('@/stores/workspaceReset', () => ({ resetWorkspaceScopedState: vi.fn(async () => {}) }))
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ resetToDefault: vi.fn(), loadThemeFromUser: vi.fn() }),
}))

function httpError(status: number, config: InternalAxiosRequestConfig): AxiosError {
  const response = { status, statusText: '', data: {}, headers: {}, config } as AxiosResponse
  return new AxiosError('failed', 'ERR_BAD_REQUEST', config, null, response)
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.resetModules()
  setActivePinia(createPinia())
  router.push.mockClear()
  sessionStorage.clear()
  window.history.replaceState({}, '', '/tickets')
  document.cookie = 'csrf_token=first; path=/'
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.useRealTimers()
  document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT'
})

/** Boot the web platform signed in, with every API request refused and the
 *  refresh rejected. Returns the URLs the API client was asked for. */
async function bootRejected() {
  const axios = (await import('axios')).default
  vi.spyOn(axios, 'post').mockImplementation((async () => {
    throw new AxiosError('refused', 'ERR_BAD_REQUEST', undefined, null, {
      status: 401,
    } as AxiosResponse)
  }) as never)
  const { configurePlatform } = await import('@/platform')
  await configurePlatform()
  const { default: apiClient } = await import('@nosdesk/core/apiClient')
  const sent: string[] = []
  apiClient.defaults.adapter = async (config) => {
    sent.push(`${config.method?.toUpperCase()} ${config.url}`)
    throw httpError(401, config)
  }
  const { useAuthStore } = await import('@/stores/auth')
  const auth = useAuthStore()
  auth.user = { uuid: 'u-1' } as never
  return { apiClient, auth, sent }
}

describe('a session the server rejects', () => {
  it('is torn down locally without asking the server to sign out', async () => {
    const { apiClient, auth, sent } = await bootRejected()

    await expect(apiClient.get('/tickets')).rejects.toBeTruthy()
    await vi.advanceTimersByTimeAsync(1000)

    expect(auth.user).toBeNull()
    expect(router.push).toHaveBeenCalled()
    expect(sent.filter((s) => s.includes('/auth/logout'))).toEqual([])
  })

  it('is torn down locally when loading the profile finds it gone', async () => {
    const { auth, sent } = await bootRejected()

    await auth.fetchUserData({ force: true }).catch(() => {})
    await vi.advanceTimersByTimeAsync(1000)

    expect(auth.user).toBeNull()
    expect(sent.filter((s) => s.includes('/auth/logout'))).toEqual([])
  })
})
