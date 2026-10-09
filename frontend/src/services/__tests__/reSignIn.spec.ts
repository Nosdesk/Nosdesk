import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

// The session expires, the app signs the person out, and they sign in again
// by passkey (which sets the user directly). The new session must behave like
// any other: an expired access token refreshes, and a later rejection signs
// them out again.

const router = vi.hoisted(() => ({ push: vi.fn() }))
vi.mock('@/router', () => ({ default: router, landAfterLogin: vi.fn() }))
vi.mock('@/stores/workspaceReset', () => ({ resetWorkspaceScopedState: vi.fn(async () => {}) }))
vi.mock('@/stores/theme', () => ({ useThemeStore: () => ({ resetToDefault: vi.fn() }) }))
vi.mock('@nosdesk/core/services/authService', () => ({
  default: { logout: vi.fn(async () => ({ logoutUrl: undefined })) },
}))

function httpError(status: number, config: InternalAxiosRequestConfig): AxiosError {
  const response = { status, statusText: '', data: {}, headers: {}, config } as AxiosResponse
  return new AxiosError('failed', 'ERR_BAD_REQUEST', config, null, response)
}

const ok = (config: InternalAxiosRequestConfig) =>
  ({ status: 200, statusText: '', data: { ok: true }, headers: {}, config }) as AxiosResponse

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

describe('signing in again by passkey after the session was rejected', () => {
  it('refreshes an expired token, and a later rejection signs out again', async () => {
    const axios = (await import('axios')).default
    let refreshRenews = false
    const refresh = vi.spyOn(axios, 'post').mockImplementation((async () => {
      if (refreshRenews) return { status: 200, data: {} }
      throw new AxiosError('refused', 'ERR_BAD_REQUEST', undefined, null, {
        status: 401,
      } as AxiosResponse)
    }) as never)
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { default: apiClient } = await import('@nosdesk/core/apiClient')
    const { useAuthStore } = await import('@/stores/auth')
    const auth = useAuthStore()
    auth.user = { uuid: 'u-1' } as never

    // Every request is refused until told otherwise.
    let accessValid = false
    apiClient.defaults.adapter = async (config) => {
      if (!accessValid) throw httpError(401, config)
      return ok(config)
    }

    // The session expires: the refresh is rejected and the app signs out.
    await expect(apiClient.get('/tickets')).rejects.toBeTruthy()
    await vi.advanceTimersByTimeAsync(1000)
    expect(auth.user).toBeNull()
    expect(router.push).toHaveBeenCalledTimes(1)

    // Signed in again by passkey: the user is set directly and the server
    // sets a new CSRF cookie.
    document.cookie = 'csrf_token=second; path=/'
    auth.user = { uuid: 'u-1' } as never

    // The access token expires: the request refreshes and is retried.
    refreshRenews = true
    refresh.mockClear()
    apiClient.defaults.adapter = async (config) => {
      if (!accessValid) {
        accessValid = true
        throw httpError(401, config)
      }
      return ok(config)
    }
    await expect(apiClient.get('/tickets')).resolves.toMatchObject({ data: { ok: true } })
    expect(refresh).toHaveBeenCalledTimes(1)

    // Later the session is revoked: the person is signed out again.
    refreshRenews = false
    accessValid = false
    apiClient.defaults.adapter = async (config) => {
      throw httpError(401, config)
    }
    await expect(apiClient.get('/tickets')).rejects.toBeTruthy()
    await vi.advanceTimersByTimeAsync(1000)
    expect(auth.user).toBeNull()
    expect(router.push).toHaveBeenCalledTimes(2)
  })
})
