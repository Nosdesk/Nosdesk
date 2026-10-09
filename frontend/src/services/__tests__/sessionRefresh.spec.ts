import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

// The web app's session refresh. Only a refresh the server refuses ends the
// session; one that can't reach the server (offline, a 502 while it restarts)
// signs nobody out.

// As the real one ends: on the login page.
const logout = vi.hoisted(() =>
  vi.fn(async () => {
    window.history.replaceState({}, '', '/login')
  }),
)
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ logout }) }))

/** An axios error carrying an HTTP status, or none for a network failure. */
function httpError(status: number | null, config = {} as InternalAxiosRequestConfig): AxiosError {
  const response =
    status === null
      ? undefined
      : ({ status, statusText: '', data: {}, headers: {}, config } as AxiosResponse)
  return new AxiosError('failed', status === null ? 'ERR_NETWORK' : 'ERR_BAD_REQUEST', config, null, response)
}

const CSRF_EXPIRED = 'expires=Thu, 01 Jan 1970 00:00:00 GMT'

beforeEach(() => {
  vi.useFakeTimers()
  vi.resetModules()
  logout.mockClear()
  sessionStorage.clear()
  window.history.replaceState({}, '', '/tickets')
  // Signed in: the server set the JS-readable CSRF cookie.
  document.cookie = 'csrf_token=first; path=/'
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
  document.cookie = `csrf_token=; path=/; ${CSRF_EXPIRED}`
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

describe('signing in again after the session was rejected', () => {
  it('holds a session again, whatever way the person signed in', async () => {
    const axios = (await import('axios')).default
    vi.spyOn(axios, 'post').mockImplementation((() => Promise.reject(httpError(401))) as never)
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { refreshSession, sessionGone } = await import('@nosdesk/core/services/session')

    expect(await refreshSession()).toBe('rejected')
    expect(sessionGone()).toBe(true)

    // A passkey sign-in sets the user directly; the server sets a new CSRF
    // cookie. Nothing else is told.
    document.cookie = 'csrf_token=second; path=/'
    expect(sessionGone()).toBe(false)
  })
})

describe('a session lost during a deliberate sign-out', () => {
  it('does not start a second sign-out', async () => {
    const { setLoggingOut } = await import('@/services/apiConfig')
    const { redirectToLogin } = await import('@/services/sessionLost')
    setLoggingOut(true)

    redirectToLogin()
    await vi.advanceTimersByTimeAsync(1000)

    expect(logout).not.toHaveBeenCalled()
    setLoggingOut(false)
  })
})

describe('a raw API request retried after a refresh', () => {
  it('goes to the workspace it was first sent to, even if the person switched meanwhile', async () => {
    const { setActiveWorkspaceSlug } = await import('@/services/activeWorkspace')
    setActiveWorkspaceSlug('acme')
    const axios = (await import('axios')).default
    vi.spyOn(axios, 'post').mockImplementation((async () => {
      // The person switches workspace while the refresh is out.
      setActiveWorkspaceSlug('globex')
      return { status: 200, data: {} }
    }) as never)
    await import('@/services/transport')
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(new Response(null, { status: 200 }))
    vi.stubGlobal('fetch', fetch)
    const { authFetch } = await import('@/services/authFetch')

    await authFetch('/sync/push', { method: 'POST', body: '[]' })

    const workspace = (call: number) =>
      ((fetch.mock.calls[call][1] as RequestInit).headers as Record<string, string>)['X-Nosdesk-Workspace']
    expect(fetch).toHaveBeenCalledTimes(2)
    expect(workspace(0)).toBe('acme')
    expect(workspace(1)).toBe('acme')
  })
})

describe('a session the server keeps rejecting', () => {
  it('signs the person out once, however many requests are refused', async () => {
    const axios = (await import('axios')).default
    const post = vi.spyOn(axios, 'post').mockImplementation((() => Promise.reject(httpError(401))) as never)
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { refreshSession } = await import('@nosdesk/core/services/session')

    // A sync push retried on its backoff, refused each time.
    for (let i = 0; i < 4; i++) {
      expect(await refreshSession()).toBe('rejected')
      await vi.advanceTimersByTimeAsync(30_000)
    }

    expect(post).toHaveBeenCalledTimes(4)
    expect(logout).toHaveBeenCalledTimes(1)
  })
})

describe('a CSRF cookie that expired while the session is still good', () => {
  it('is renewed by the server rather than read as signed out', async () => {
    document.cookie = `csrf_token=; path=/; ${CSRF_EXPIRED}`
    const axios = (await import('axios')).default
    const post = vi.spyOn(axios, 'post').mockImplementation((async () => {
      // The server's answer sets a fresh CSRF cookie.
      document.cookie = 'csrf_token=renewed; path=/'
      return { status: 200, data: {} }
    }) as never)
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { refreshSession, sessionGone } = await import('@nosdesk/core/services/session')

    expect(await refreshSession()).toBe('renewed')
    expect(post).toHaveBeenCalledTimes(1)
    expect(sessionGone()).toBe(false)
    expect(logout).not.toHaveBeenCalled()
  })
})

describe('an API request retried after a refresh', () => {
  it('goes to the workspace it was first sent to, even if the person switched meanwhile', async () => {
    const { setActiveWorkspaceSlug } = await import('@/services/activeWorkspace')
    setActiveWorkspaceSlug('acme')
    const axios = (await import('axios')).default
    vi.spyOn(axios, 'post').mockImplementation((async () => {
      // The person switches workspace while the refresh is out.
      setActiveWorkspaceSlug('globex')
      return { status: 200, data: {} }
    }) as never)
    const { configurePlatform } = await import('@/platform')
    await configurePlatform()
    const { default: apiClient } = await import('@nosdesk/core/apiClient')
    const sentTo: Array<string | undefined> = []
    apiClient.defaults.adapter = async (config) => {
      sentTo.push(config.headers.get('X-Nosdesk-Workspace') as string | undefined)
      if (sentTo.length === 1) throw httpError(401, config)
      return { status: 200, statusText: '', data: {}, headers: {}, config } as AxiosResponse
    }

    // The answer belongs to the old workspace, so it is not delivered.
    await apiClient.post('/tickets/7/comments', { body: 'hi' }).catch(() => {})

    expect(sentTo).toEqual(['acme', 'acme'])
  })
})
