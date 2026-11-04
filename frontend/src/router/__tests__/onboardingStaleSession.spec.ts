import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// First-run setup in a browser that still holds a session cookie from an
// earlier install (or another Nosdesk on the same host). The server has no
// accounts, so nobody is signed in, and the tab must not act as if someone
// were: asking for the profile or loading plugins on that cookie fails, and
// the refresh that follows is refused.
// The real route table and guards run here, over the web transport.

vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))

vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => 'host',
}))
const service = vi.hoisted(() => ({
  checkSetupStatus: vi.fn(),
  getCurrentUser: vi.fn(),
}))
vi.mock('@nosdesk/core/services/authService', async (importOriginal) => {
  const actual = await importOriginal<{ default: object }>()
  return { ...actual, default: { ...actual.default, ...service } }
})
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: async () => {},
  enterWorkspace: async () => {},
}))
vi.mock('@/stores/theme', () => ({
  useThemeStore: () => ({ resetToDefault: vi.fn(), loadThemeFromUser: vi.fn() }),
}))
vi.mock('@nosdesk/core/stores/featureFlags', () => ({
  useFeatureFlagsStore: () => ({ loaded: true, loading: false, load: async () => {} }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ loaded: true, loading: false, load: async () => {} }),
}))
const sync = vi.hoisted(() => ({ ensureSyncRuntime: vi.fn(async () => {}) }))
vi.mock('@/sync/lifecycle', () => sync)

import '@/services/transport'
import router from '@/router'
import { useAuthStore } from '@/stores/auth'

const fresh = { requires_setup: true, user_count: 0, microsoft_auth_enabled: false, oidc_enabled: false }

function setCookie(value: string) {
  document.cookie = `csrf_token=${value}; path=/`
}
function clearCookie() {
  document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT'
}

beforeEach(async () => {
  vi.clearAllMocks()
  clearCookie()
  localStorage.clear()
  // Each test starts on the sign-in page, signed out.
  setActivePinia(createPinia())
  await router.replace('/login')
})
afterEach(clearCookie)

/** Load the page with `cookie` held, the way a browser tab would. */
function loadPage(cookie: string | null) {
  if (cookie) setCookie(cookie)
  setActivePinia(createPinia())
  return useAuthStore()
}

describe('first-run setup with a session cookie from an earlier install', () => {
  it('opens the setup page signed out, without asking for a profile', async () => {
    service.checkSetupStatus.mockResolvedValue(fresh)
    const auth = loadPage('left-over')
    expect(auth.isAuthenticated).toBe(true)

    await router.push('/onboarding')

    expect(router.currentRoute.value.name).toBe('onboarding')
    expect(auth.isAuthenticated).toBe(false)
    expect(service.getCurrentUser).not.toHaveBeenCalled()
    expect(sync.ensureSyncRuntime).not.toHaveBeenCalled()
    expect(document.cookie).not.toContain('csrf_token=left-over')
  })

  it('sends the dashboard to setup, signed out', async () => {
    service.checkSetupStatus.mockResolvedValue(fresh)
    const auth = loadPage('left-over')

    await router.push('/')

    expect(router.currentRoute.value.name).toBe('onboarding')
    expect(auth.isAuthenticated).toBe(false)
    expect(service.getCurrentUser).not.toHaveBeenCalled()
  })

  it('keeps a session another tab signed in to since the page loaded', async () => {
    const auth = loadPage('left-over')
    setCookie('new-sign-in')

    await auth.forgetStaleSession()

    expect(auth.isAuthenticated).toBe(false)
    expect(document.cookie).toContain('csrf_token=new-sign-in')
  })

  it('keeps the session when the server could not be asked', async () => {
    service.checkSetupStatus.mockResolvedValue({ ...fresh, assumed: true })
    service.getCurrentUser.mockRejectedValue(new Error('offline'))
    const auth = loadPage('maybe-live')

    await router.push('/onboarding')

    expect(auth.isAuthenticated).toBe(true)
    expect(document.cookie).toContain('csrf_token=maybe-live')
  })
})
