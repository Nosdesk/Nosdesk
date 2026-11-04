import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { isCancel, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

// Signing out sends nothing for the session but /auth/logout. The signed-in
// views are still mounted while their caches are dropped, and each mounted
// query refetches at once; those requests are held and cancelled unsent rather
// than sent to come back 401.

// A navigation the test can hold open, standing in for the views unmounting.
const navigation = vi.hoisted(() => ({ finish: () => {} }))
const router = vi.hoisted(() => ({
  push: vi.fn(
    (to: unknown) =>
      new Promise<void>((resolve) => {
        navigation.finish = () => {
          window.history.replaceState({}, '', typeof to === 'string' ? to : '/login')
          resolve()
        }
      }),
  ),
  replace: vi.fn(),
  currentRoute: { value: { fullPath: '/tickets', name: 'tickets', query: {} } },
}))
vi.mock('@/router', () => ({ default: router, landAfterLogin: vi.fn() }))
vi.mock('@/stores/workspaceReset', () => ({ resetWorkspaceScopedState: vi.fn(async () => {}) }))
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ resetToDefault: vi.fn(), loadThemeFromUser: vi.fn() }),
}))

function ok(config: InternalAxiosRequestConfig): AxiosResponse {
  return { status: 200, statusText: '', data: {}, headers: {}, config } as AxiosResponse
}

beforeEach(() => {
  vi.resetModules()
  setActivePinia(createPinia())
  router.push.mockClear()
  window.history.replaceState({}, '', '/tickets')
  // Signed in: the server set the JS-readable CSRF cookie.
  document.cookie = 'csrf_token=first; path=/'
})
afterEach(() => {
  vi.restoreAllMocks()
  document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT'
})

/** Boot the web platform signed in. Every request the API client sends is
 *  recorded and answered 200. */
async function bootSignedIn() {
  const { configurePlatform } = await import('@/platform')
  await configurePlatform()
  const { default: apiClient } = await import('@nosdesk/core/apiClient')
  const sent: string[] = []
  apiClient.defaults.adapter = async (config) => {
    sent.push(`${config.method?.toUpperCase()} ${config.url}`)
    if (config.url?.includes('/auth/logout')) {
      // As the server does: the session's cookies go with it.
      document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT'
    }
    return ok(config)
  }
  const { useAuthStore } = await import('@/stores/auth')
  const auth = useAuthStore()
  auth.user = { uuid: 'u-1' } as never
  return { apiClient, auth, sent }
}

/** Run until the sign-out is waiting on the navigation to the login page. */
async function untilNavigating() {
  await vi.waitFor(() => expect(router.push).toHaveBeenCalled())
}

/** Let a request just made reach the request gates, as the views' refetches
 *  do long before the navigation commits. */
const reachGates = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('signing out', () => {
  it('covers the signed-in views from the teardown until the login page has taken over', async () => {
    const { apiClient, auth } = await bootSignedIn()
    let signingOutDuringLogout: boolean | null = null
    const send = apiClient.defaults.adapter as (c: InternalAxiosRequestConfig) => Promise<AxiosResponse>
    apiClient.defaults.adapter = async (config) => {
      if (config.url?.includes('/auth/logout')) signingOutDuringLogout = auth.signingOut
      return send(config)
    }

    const done = auth.logout()
    await untilNavigating()
    // The page stays as it is while the server ends the session...
    expect(signingOutDuringLogout).toBe(false)
    // ...then is covered, signed out, while the views give way to the login page.
    expect(auth.user).toBeNull()
    expect(auth.signingOut).toBe(true)

    navigation.finish()
    await done
    expect(auth.signingOut).toBe(false)
  })

  it('covers them the same way when the session was lost', async () => {
    const { auth } = await bootSignedIn()
    const done = auth.sessionLost()
    await untilNavigating()
    expect(auth.signingOut).toBe(true)
    navigation.finish()
    await done
    expect(auth.signingOut).toBe(false)
  })

  it('is signed out at once, and signed in again by the next sign-in', async () => {
    const { auth } = await bootSignedIn()
    expect(auth.isAuthenticated).toBe(true)

    const signingOut = auth.logout()
    await untilNavigating()
    expect(auth.isAuthenticated).toBe(false)
    navigation.finish()
    await signingOut
    expect(auth.isAuthenticated).toBe(false)

    auth.user = { uuid: 'u-2' } as never
    expect(auth.isAuthenticated).toBe(true)
  })

  it('sends nothing but /auth/logout, and cancels what the views asked for meanwhile', async () => {
    const { apiClient, auth, sent } = await bootSignedIn()

    const signingOut = auth.logout()
    await untilNavigating()
    // A view still mounted refetches as its cache is dropped.
    const refetch = apiClient.get('/me/workspaces').then(
      () => 'answered',
      (e: unknown) => (isCancel(e) ? 'cancelled' : 'failed'),
    )
    await reachGates()
    expect(sent).toEqual(['POST /auth/logout'])

    navigation.finish()
    await signingOut
    expect(await refetch).toBe('cancelled')
    expect(sent).toEqual(['POST /auth/logout'])
  })

  it("lets the sign-in page's own requests through once it shows", async () => {
    const { apiClient, auth, sent } = await bootSignedIn()

    const signingOut = auth.logout()
    await untilNavigating()
    window.history.replaceState({}, '', '/login')
    await apiClient.post('/auth/passkeys/login/start')
    expect(sent).toContain('POST /auth/passkeys/login/start')

    navigation.finish()
    await signingOut
  })

  it('sends again once the sign-out has finished', async () => {
    const { apiClient, auth, sent } = await bootSignedIn()

    const signingOut = auth.logout()
    await untilNavigating()
    navigation.finish()
    await signingOut

    await apiClient.get('/branding')
    expect(sent).toContain('GET /branding')
  })

  it('releases what needs the session before revoking it', async () => {
    const { apiClient, auth, sent } = await bootSignedIn()
    const { transport } = await import('@nosdesk/core/transport')
    const strategy = transport().auth as { beforeSignOut?: () => Promise<void> }
    // As mobile does: give up the device's push registration.
    strategy.beforeSignOut = async () => {
      await apiClient.delete('/notifications/devices/device-token')
    }

    const signingOut = auth.logout()
    await untilNavigating()
    navigation.finish()
    await signingOut

    expect(sent).toEqual(['DELETE /notifications/devices/device-token', 'POST /auth/logout'])
    delete strategy.beforeSignOut
  })
})

describe('a session the server rejected', () => {
  it('cancels what the views asked for while it is torn down', async () => {
    const { apiClient, auth, sent } = await bootSignedIn()

    const losing = auth.sessionLost()
    await untilNavigating()
    expect(auth.isAuthenticated).toBe(false)
    const refetch = apiClient.get('/tickets/recent').then(
      () => 'answered',
      (e: unknown) => (isCancel(e) ? 'cancelled' : 'failed'),
    )
    await reachGates()

    navigation.finish()
    await losing
    expect(await refetch).toBe('cancelled')
    expect(sent).toEqual([])
  })
})
