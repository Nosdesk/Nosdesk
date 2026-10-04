import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The form edits the workspace's saved settings, so it reads them from the
// admin endpoint. The public branding read answers the built-in defaults
// (note off) until the session is up, which seeded the form wrongly.
const branding = vi.hoisted(() => ({
  admin: vi.fn(),
  public: vi.fn(),
}))
vi.mock('@nosdesk/core/services/brandingService', () => ({
  default: {
    getAdminBrandingConfig: branding.admin,
    getBrandingConfig: branding.public,
    updateBrandingConfig: vi.fn(),
  },
}))

import EmailSecurityNoteForm from '@/components/admin/email/EmailSecurityNoteForm.vue'

const SAVED = {
  app_name: 'Acme IT',
  logo_url: null,
  logo_light_url: null,
  favicon_url: null,
  primary_color: '#c32222',
  updated_at: '2026-10-04T00:00:00Z',
  signature_default: null,
  channel_auto_ack_enabled: true,
  channel_auto_ack_template: null,
  email_security_note_enabled: true,
  email_security_note_template: 'Acme IT only emails you from {{domain}}.',
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

async function mountForm() {
  wrapper = mountWithProviders(EmailSecurityNoteForm, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

describe('EmailSecurityNoteForm', () => {
  it("shows the workspace's saved note, read from the admin settings", async () => {
    branding.admin.mockResolvedValue(SAVED)
    await mountForm()

    expect(branding.public).not.toHaveBeenCalled()
    expect(wrapper!.get('[role="switch"]').attributes('aria-checked')).toBe('true')
    const note = wrapper!.get('textarea').element as HTMLTextAreaElement
    expect(note.value).toBe(SAVED.email_security_note_template)
    expect(note.disabled).toBe(false)
  })

  it('shows a load error instead of a switched-off note when the read fails', async () => {
    branding.admin.mockRejectedValue(new Error('Request failed with status code 500'))
    await mountForm()

    expect(wrapper!.text()).toContain('admin-branding-error-load')
    expect(wrapper!.find('form').exists()).toBe(false)
  })
})
