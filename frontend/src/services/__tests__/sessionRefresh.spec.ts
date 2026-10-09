import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

// The web app's session refresh. Only a refresh the server refuses ends the
// session; one that can't reach the server (offline, a 502 while it restarts)
// signs nobody out.

const logout = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ logout }) }))

/** An axios error carrying an HTTP status, or none for a network failure. */
function httpError(status: number | null, config = {} as InternalAxiosRequestConfig): AxiosError {
  const response =
    status === null
      ? undefined
      : ({ status, statusText: '', data: {}, headers: {}, config } as AxiosResponse)
  return new AxiosError('failed', status === null ? 'ERR_NETWORK' : 'ERR_BAD_REQUEST', config, null, response)
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.resetModules()
  logout.mockClear()
  sessionStorage.clear()
  window.history.replaceState({}, '', '/tickets')
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe('refreshing the session', () => {
  async function refreshAnswered(answer: () => Promise<unknown>) {
    const axios = (await import('axios')).default
    vi.spyOn(axios, 'post').mockImplementation(answer as never)
    await import('@/services/transport')
    const { refreshAccessToken } = await import('@nosdesk/core/services/authRefresh')
    return refreshAccessToken()
  }

  it('is renewed when the server answers', async () => {
    expect(await refreshAnswered(async () => ({ status: 200, data: {} }))).toBe('renewed')
  })

  it('is rejected when the server refuses the session', async () => {
    expect(await refreshAnswered(() => Promise.reject(httpError(401)))).toBe('rejected')
  })

  it('is unavailable, not rejected, when offline or the server is failing', async () => {
    expect(await refreshAnswered(() => Promise.reject(httpError(null)))).toBe('unavailable')
    expect(await refreshAnswered(() => Promise.reject(httpError(502)))).toBe('unavailable')
  })
})

describe('an API request after the access token expired', () => {
  /** Boot the web platform with every API request answered 401 and the
   *  refresh answered by `refresh`. */
  async function bootWeb(refresh: () => Promise<unknown>) {
    const axios = (await import('axios')).default
    const post = vi.spyOn(axios, 'post').mockImplementation(refresh as never)
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { default: apiClient } = await import('@nosdesk/core/apiClient')
    apiClient.defaults.adapter = async (config) => {
      throw httpError(401, config)
    }
    return { apiClient, post }
  }

  it('does not sign the person out when the refresh cannot reach the server', async () => {
    const { apiClient } = await bootWeb(() => Promise.reject(httpError(null)))

    await expect(apiClient.get('/tickets')).rejects.toBeTruthy()
    await vi.advanceTimersByTimeAsync(1000)

    expect(logout).not.toHaveBeenCalled()
    expect(sessionStorage.getItem('redirecting-to-login')).toBeNull()
  })

  it('signs the person out once when the refresh is rejected', async () => {
    const { apiClient } = await bootWeb(() => Promise.reject(httpError(401)))

    await Promise.allSettled([apiClient.get('/tickets'), apiClient.get('/users')])
    await vi.advanceTimersByTimeAsync(1000)

    expect(logout).toHaveBeenCalledTimes(1)
  })
})
