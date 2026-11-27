import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The server's answer, the same for every address.
const answer = vi.hoisted(() => ({ verification_required: false, message: 'Thanks.' }))
vi.mock('@nosdesk/core/services/publicService', () => ({
  publicService: {
    getFormChallenge: async () => ({}),
    submitTicket: vi.fn(async () => ({ ...answer })),
    searchDocs: async () => [],
    uploadAttachment: vi.fn(),
  },
}))
vi.mock('@nosdesk/core/utils/formChallenge', () => ({
  solveFormChallenge: async () => ({ nonce: 'n' }),
}))
vi.mock('@nosdesk/core/stores/publicSettings', () => ({
  usePublicSettingsStore: () => ({
    settings: {
      guest_tickets_enabled: true,
      guest_ticket_lookup_enabled: true,
      guest_ticket_attachments_enabled: false,
      request_types: [],
    },
    load: async () => {},
  }),
}))
vi.mock('@/components/public/RequesterLink.vue', () => ({
  default: { props: ['to'], template: '<a :href="to" data-test="sign-in"><slot /></a>' },
}))
vi.mock('@/views/public/PublicLayout.vue', () => ({ default: { template: '<div><slot /></div>' } }))

import GuestTicketSubmitView from '@/views/public/GuestTicketSubmitView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  vi.restoreAllMocks()
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function submit(verificationRequired: boolean) {
  answer.verification_required = verificationRequired
  wrapper = mountWithProviders(GuestTicketSubmitView)
  await flushPromises()
  const inputs = wrapper.findAll('input:not([name="website"])')
  await inputs[0]!.setValue('Sam')
  await inputs[1]!.setValue('sam@example.com')
  await inputs[2]!.setValue('Printer jammed')
  await wrapper.find('textarea').setValue('It jams on every page.')
  // The form waits out its challenge's minimum age before sending.
  const later = Date.now() + 60_000
  vi.spyOn(Date, 'now').mockReturnValue(later)
  await wrapper.find('form').trigger('submit')
  await flushPromises()
}

describe('GuestTicketSubmitView answer', () => {
  it('with confirmation off shows no ticket number or status link, and points account holders to sign in', async () => {
    await submit(false)
    const text = wrapper!.text()
    expect(text).toContain('guest-submit-received-title')
    expect(text).toContain('guest-submit-have-account-hint')
    expect(text).not.toContain('#')
    expect(wrapper!.find('[data-test="sign-in"]').attributes('href')).toBe('/login')
    expect(wrapper!.find('code').exists()).toBe(false)
  })

  it('with confirmation on asks for the email and points account holders to sign in', async () => {
    await submit(true)
    const text = wrapper!.text()
    expect(text).toContain('guest-submit-verify-title')
    expect(text).toContain('guest-submit-have-account-hint')
    expect(wrapper!.find('[data-test="sign-in"]').attributes('href')).toBe('/login')
  })
})
