import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('vue-router', () => ({
  useRoute: () => ({ name: 'admin-guest', params: {} }),
  useRouter: () => ({ push: vi.fn() }),
  onBeforeRouteLeave: vi.fn(),
}))
// What the next `get()` returns on top of the base settings.
const loaded = vi.hoisted(() => ({ overrides: {} as Record<string, unknown> }))
vi.mock('@nosdesk/core/services/publicService', () => ({
  adminGuestSettingsService: {
    get: async () => ({
      guest_ticket_enabled: true,
      guest_ticket_email_verification: false,
      guest_ticket_attachments_enabled: false,
      guest_ticket_default_priority: 'medium',
      guest_ticket_rate_limit_per_hour: 10,
      portal_share_by_domain: false,
      portal_share_staff_requests: false,
      ...loaded.overrides,
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
import { adminGuestSettingsService } from '@nosdesk/core/services/publicService'

let wrapper: VueWrapper | null = null
afterEach(() => {
  loaded.overrides = {}
  vi.mocked(adminGuestSettingsService.update).mockReset()
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

describe('GuestAccessSettingsView staff requests toggle', () => {
  // The switch whose label is `key` (translations resolve to the id here).
  function switchFor(key: string) {
    const label = wrapper!.findAll('label').find((l) => l.text() === key)
    expect(label, key).toBeTruthy()
    return wrapper!.find(`#${label!.attributes('for')}`)
  }

  it('is off and unavailable while sharing is off', async () => {
    wrapper = mountWithProviders(GuestAccessSettingsView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const staff = switchFor('admin-guest-toggle-share-staff-label')
    expect(staff.attributes('aria-checked')).toBe('false')
    expect(staff.attributes('disabled')).toBeDefined()
  })

  it('can be turned on once sharing is on', async () => {
    loaded.overrides = { portal_share_by_domain: true }
    wrapper = mountWithProviders(GuestAccessSettingsView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const staff = switchFor('admin-guest-toggle-share-staff-label')
    expect(staff.attributes('disabled')).toBeUndefined()
    await staff.trigger('click')
    expect(staff.attributes('aria-checked')).toBe('true')
  })
})

describe('GuestAccessSettingsView organisation domains', () => {
  function mountView() {
    wrapper = mountWithProviders(GuestAccessSettingsView, {}, {}, [[PiniaColada, {}] as never])
    return flushPromises()
  }
  const field = () => wrapper!.find<HTMLTextAreaElement>('#guest-organisation-domains')
  const saveButton = () =>
    wrapper!.findAll('button').find((b) => b.text() === 'admin-guest-save')!

  it('shows the listed domains one per line, and no sign-in note without a provider', async () => {
    loaded.overrides = { organisation_domains: ['acme.com', 'it.acme.com'], sign_in_domains: [] }
    await mountView()
    expect(field().element.value).toBe('acme.com\nit.acme.com')
    expect(wrapper!.find('[data-test="sign-in-domains"]').exists()).toBe(false)
    expect(saveButton().attributes('disabled')).toBeDefined()
  })

  it("notes the sign-in provider's domains as already counted", async () => {
    loaded.overrides = { organisation_domains: [], sign_in_domains: ['sso.acme.com'] }
    await mountView()
    expect(wrapper!.find('[data-test="sign-in-domains"]').exists()).toBe(true)
  })

  it('saves the typed domains as a list', async () => {
    loaded.overrides = { organisation_domains: ['acme.com'], sign_in_domains: [] }
    await mountView()
    vi.mocked(adminGuestSettingsService.update).mockImplementation(async (update) => ({
      ...(await adminGuestSettingsService.get()),
      organisation_domains: update.organisation_domains ?? [],
    }))
    await field().setValue('acme.com\n\n  beta.example  \n')
    expect(saveButton().attributes('disabled')).toBeUndefined()
    await saveButton().trigger('click')
    await flushPromises()
    expect(adminGuestSettingsService.update).toHaveBeenCalledWith(
      expect.objectContaining({ organisation_domains: ['acme.com', 'beta.example'] }),
    )
    expect(field().element.value).toBe('acme.com\nbeta.example')
    expect(saveButton().attributes('disabled')).toBeDefined()
  })

  it("shows the server's reason when it refuses a domain", async () => {
    await mountView()
    vi.mocked(adminGuestSettingsService.update).mockRejectedValue({
      response: {
        status: 400,
        data: { error: 'gmail.com is a personal email provider', code: 'BAD_REQUEST' },
        config: {},
      },
    })
    await field().setValue('gmail.com')
    await saveButton().trigger('click')
    await flushPromises()
    expect(wrapper!.text()).toContain('gmail.com is a personal email provider')
    // The edit stays, unsaved.
    expect(field().element.value).toBe('gmail.com')
    expect(saveButton().attributes('disabled')).toBeUndefined()
  })
})
