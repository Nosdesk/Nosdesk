import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const auth = vi.hoisted(() => ({
  getUserAuthIdentities: vi.fn(),
  connectOAuthProvider: vi.fn(),
  deleteUserAuthIdentity: vi.fn(),
}))
vi.mock('@nosdesk/core/services/authService', () => ({ default: auth }))
vi.mock('@/services/userService', () => ({ default: {} }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: { uuid: 'me' } }) }))

import AuthMethodsSettings from '@/components/settings/AuthMethodsSettings.vue'

let wrapper: VueWrapper | null = null
let successes: string[] = []
let errors: string[] = []

/** Mount the card as if the browser were at `address`. */
async function openAt(address: string) {
  window.history.replaceState(null, '', address)
  successes = []
  errors = []
  wrapper = mountWithProviders(AuthMethodsSettings, {
    onSuccess: (m: string) => successes.push(m),
    onError: (m: string) => errors.push(m),
  })
  await flushPromises()
  return wrapper
}

const address = () => `${window.location.pathname}${window.location.search}`

beforeEach(() => {
  auth.getUserAuthIdentities.mockResolvedValue([
    { id: 1, provider_type: 'local', email: 'me@example.com', created_at: '2026-11-01T00:00:00Z' },
  ])
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
  window.history.replaceState(null, '', '/')
})

describe('AuthMethodsSettings connecting Microsoft', () => {
  it('asks to come back to this page', async () => {
    auth.connectOAuthProvider.mockResolvedValue({})
    await openAt('/profile/settings/security')
    const connect = wrapper!
      .findAll('button')
      .find((b) => b.text().includes('settings-auth-methods-connect-microsoft'))
    await connect!.trigger('click')
    await flushPromises()

    expect(auth.connectOAuthProvider).toHaveBeenCalledWith('microsoft', '/profile/settings/security')
  })

  it('says a connect that came back succeeded, shows the list fresh and clears the address', async () => {
    await openAt('/profile/settings/security?auth_success=true&keep=1')

    expect(successes).toEqual(['settings-auth-methods-link-success'])
    expect(errors).toEqual([])
    expect(auth.getUserAuthIdentities).toHaveBeenCalled()
    expect(address()).toBe('/profile/settings/security?keep=1')
  })

  it('says a connect that came back refused failed and clears the address', async () => {
    await openAt('/profile/settings/security?auth_error=connect_failed')

    expect(errors).toEqual(['settings-auth-methods-link-error'])
    expect(successes).toEqual([])
    expect(address()).toBe('/profile/settings/security')
  })

  it('says nothing without a connect result, and leaves other errors alone', async () => {
    await openAt('/profile/settings/security?auth_error=something_else')

    expect(successes).toEqual([])
    expect(errors).toEqual([])
    expect(address()).toBe('/profile/settings/security?auth_error=something_else')
  })
})
