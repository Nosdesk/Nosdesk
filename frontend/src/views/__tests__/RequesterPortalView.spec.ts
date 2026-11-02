import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const handoff = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/workspacePortalService', () => ({ requestPortalHandoff: handoff }))
const openExternalUrl = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/platform', () => ({ openExternalUrl }))
const logout = vi.hoisted(() => vi.fn())
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ logout }) }))
vi.mock('@/components/auth/AuthLayout.vue', () => ({ default: { template: '<div><slot /></div>' } }))

import RequesterPortalView from '@/views/RequesterPortalView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

function button(label: string) {
  return wrapper!.findAll('button').find((b) => b.text() === label)!
}

describe('a requester in the app', () => {
  it('opens the portal in the browser with a fresh one-time address', async () => {
    handoff.mockResolvedValue('https://help.example.com/api/portal/auth/handoff?t=abc&next=%2Ftickets')
    wrapper = mountWithProviders(RequesterPortalView)
    await button('requester-portal-open').trigger('click')
    await flushPromises()
    expect(handoff).toHaveBeenCalledTimes(1)
    expect(openExternalUrl).toHaveBeenCalledWith(
      'https://help.example.com/api/portal/auth/handoff?t=abc&next=%2Ftickets',
    )
  })

  it('says so when the portal could not be opened, and can sign out', async () => {
    handoff.mockRejectedValue(new Error('offline'))
    wrapper = mountWithProviders(RequesterPortalView)
    await button('requester-portal-open').trigger('click')
    await flushPromises()
    expect(openExternalUrl).not.toHaveBeenCalled()
    expect(wrapper.find('[role="alert"]').exists()).toBe(true)
    await button('requester-portal-sign-out').trigger('click')
    expect(logout).toHaveBeenCalled()
  })
})
