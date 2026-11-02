import { afterEach, describe, expect, it, vi } from 'vitest'

// A sign-in at the app that the server sent to the portal (a requester on
// self-hosted) goes there, and nowhere off this origin.

const assign = vi.fn()
vi.stubGlobal('location', { ...window.location, assign })

const post = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post,
    get: vi.fn(),
    defaults: { headers: { common: {} } },
    interceptors: { request: { use: vi.fn() }, response: { use: vi.fn() } },
  },
}))
const landAfterLogin = vi.hoisted(() => vi.fn())
vi.mock('@/router', () => ({ default: { currentRoute: { value: {} }, push: vi.fn() }, landAfterLogin }))

import { createPinia, setActivePinia } from 'pinia'
import { followPortalLanding } from '@/utils/portalLanding'
import { useAuthStore } from '@/stores/auth'

afterEach(() => vi.clearAllMocks())

describe('followPortalLanding', () => {
  it('follows a portal path on this origin', () => {
    expect(followPortalLanding({ success: true, portal: '/portal/tickets' })).toBe(true)
    expect(assign).toHaveBeenCalledWith('/portal/tickets')
  })

  it('leaves an ordinary sign-in alone and refuses other origins', () => {
    for (const data of [
      { success: true, csrf_token: 'x', user: {} },
      { portal: 'https://evil.example/' },
      { portal: '//evil.example' },
      { portal: '/\\evil.example' },
      null,
    ]) {
      expect(followPortalLanding(data)).toBe(false)
    }
    expect(assign).not.toHaveBeenCalled()
  })
})

describe('a password sign-in answered with the portal', () => {
  it('goes to the portal instead of landing in the app', async () => {
    setActivePinia(createPinia())
    post.mockResolvedValue({ data: { success: true, portal: '/portal/tickets' } })
    const auth = useAuthStore()

    expect(await auth.login({ email: 'pat@example.com', password: 'secret' })).toBe(true)
    expect(assign).toHaveBeenCalledWith('/portal/tickets')
    expect(landAfterLogin).not.toHaveBeenCalled()
    expect(auth.user).toBeNull()
  })
})
