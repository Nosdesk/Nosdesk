import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const route = vi.hoisted(() => ({ name: 'login', query: {} as Record<string, string> }))
vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ replace: vi.fn(), push: vi.fn(), currentRoute: { value: route } }),
}))
vi.mock('@/router', () => ({ landAfterLogin: vi.fn() }))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ mfaRequired: false, passkeyMfaRequired: false, clearMfaState: vi.fn() }),
}))
vi.mock('@nosdesk/core/stores/mfaSetup', () => ({ useMfaSetupStore: () => ({}) }))
vi.mock('@/stores/branding', () => ({
  useBrandingStore: () => ({ isLoaded: true, appName: 'Nosdesk', getLogoUrl: () => null }),
}))
vi.mock('@/stores/theme', () => ({ useThemeStore: () => ({ isDarkMode: false }) }))
vi.mock('@/composables/useMicrosoftAuth', () => ({
  useMicrosoftAuth: () => ({ handleMicrosoftLogin: vi.fn(), handleMicrosoftLogout: vi.fn(), error: { value: null } }),
}))
vi.mock('@/composables/usePasskeys', () => ({
  usePasskeys: () => ({
    isSupported: { value: false },
    loginWithPasskey: vi.fn(),
    loginWithPasskeyConditional: vi.fn(async () => null),
    error: { value: null },
    checkSupport: vi.fn(async () => false),
  }),
}))
// The animated hero needs a 2D canvas, which jsdom lacks.
vi.mock('@/components/auth/AuthLayout.vue', () => ({ default: { template: '<div><slot /></div>' } }))
// Hosted sign-in: SSO is the only way in, so the page starts it on load.
vi.mock('@nosdesk/core/services/authService', () => ({
  default: {
    checkSetupStatus: async () => ({
      requires_setup: false,
      oidc_enabled: true,
      local_auth_disabled: true,
      microsoft_auth_enabled: false,
    }),
  },
}))

import LoginView from '@/views/LoginView.vue'

let wrapper: VueWrapper | null = null
const fetchMock = vi.fn()

beforeEach(() => {
  fetchMock.mockResolvedValue({ ok: true, json: async () => ({ auth_url: 'about:blank' }) })
  vi.stubGlobal('fetch', fetchMock)
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

/** The body of the SSO authorize request the page sent on load. */
async function authorizeBody(query: Record<string, string>): Promise<Record<string, unknown>> {
  route.query = query
  wrapper = mountWithProviders(LoginView)
  await flushPromises()
  const call = fetchMock.mock.calls.find(([url]) => String(url).endsWith('/api/auth/oauth/authorize'))
  expect(call, 'authorize request').toBeDefined()
  return JSON.parse(String(call![1].body))
}

describe('LoginView SSO', () => {
  it('asks to come back to the page the sign-in guard was sent from', async () => {
    const body = await authorizeBody({ redirect: '/acme/tickets/12?tab=notes' })
    expect(body).toMatchObject({ provider_type: 'oidc', redirect_uri: '/acme/tickets/12?tab=notes' })
  })

  it('sends no return page that would leave the app', async () => {
    for (const redirect of ['//evil.example', '/\\evil.example', 'https://evil.example/x', 'javascript:alert(1)']) {
      const body = await authorizeBody({ redirect })
      expect(body.redirect_uri, redirect).toBeUndefined()
      wrapper?.unmount()
      wrapper = null
      fetchMock.mockClear()
    }
  })
})
