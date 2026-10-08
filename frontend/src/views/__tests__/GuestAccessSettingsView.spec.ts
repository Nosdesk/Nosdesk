import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('vue-router', () => ({
  useRoute: () => ({ name: 'admin-guest', params: {} }),
  useRouter: () => ({ push: vi.fn() }),
  onBeforeRouteLeave: vi.fn(),
}))
vi.mock('@nosdesk/core/services/publicService', () => ({
  adminGuestSettingsService: {
    get: async () => ({
      guest_ticket_enabled: true,
      guest_ticket_email_verification: false,
      guest_ticket_attachments_enabled: false,
      guest_ticket_default_priority: 'medium',
      guest_ticket_rate_limit_per_hour: 10,
    }),
    update: vi.fn(),
  },
}))
vi.mock('@/composables/useWorkspacePortal', () => ({
  useWorkspacePortal: () => ({ portalUrl: () => 'https://help.example.com' }),
}))
// The dropdown, reduced to the values it offers.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    props: ['options', 'modelValue'],
    template: `<div data-test="dropdown" :data-values="(options || []).map((o) => o.value).join(',')"></div>`,
  },
}))

import GuestAccessSettingsView from '@/views/GuestAccessSettingsView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

describe('GuestAccessSettingsView default priority', () => {
  it('offers every priority', async () => {
    wrapper = mountWithProviders(GuestAccessSettingsView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const offered = wrapper
      .findAll('[data-test="dropdown"]')
      .map((d) => d.attributes('data-values'))
      .filter((v) => v?.includes('medium'))
    expect(offered).toEqual(['urgent,high,medium,low,none'])
  })
})
