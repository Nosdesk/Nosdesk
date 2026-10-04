import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const branding = vi.hoisted(() => ({
  admin: vi.fn(),
  public: vi.fn(),
  update: vi.fn(),
}))
vi.mock('@nosdesk/core/services/brandingService', () => ({
  default: {
    getAdminBrandingConfig: branding.admin,
    getBrandingConfig: branding.public,
    updateBrandingConfig: branding.update,
  },
}))

import EmailAutoAckForm from '@/components/admin/email/EmailAutoAckForm.vue'

const SAVED = {
  app_name: 'Acme IT',
  logo_url: null,
  logo_light_url: null,
  favicon_url: null,
  primary_color: '#c32222',
  updated_at: '2026-10-04T00:00:00Z',
  signature_default: null,
  channel_auto_ack_enabled: false,
  channel_auto_ack_template: 'Hi {{customer_first_name}}, we have your request.',
  email_security_note_enabled: true,
  email_security_note_template: null,
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

async function mountForm() {
  wrapper = mountWithProviders(EmailAutoAckForm, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

describe('EmailAutoAckForm', () => {
  it("shows the workspace's saved acknowledgement, read from the admin settings", async () => {
    branding.admin.mockResolvedValue(SAVED)
    await mountForm()

    expect(branding.public).not.toHaveBeenCalled()
    expect(wrapper!.get('[role="switch"]').attributes('aria-checked')).toBe('false')
    const template = wrapper!.get('textarea').element as HTMLTextAreaElement
    expect(template.value).toBe(SAVED.channel_auto_ack_template)
    expect(template.disabled).toBe(true)
  })

  it('saves only the acknowledgement settings', async () => {
    branding.admin.mockResolvedValue(SAVED)
    branding.update.mockResolvedValue({ ...SAVED, channel_auto_ack_enabled: true })
    await mountForm()

    await wrapper!.get('[role="switch"]').trigger('click')
    await wrapper!.get('form').trigger('submit')
    await flushPromises()

    expect(branding.update).toHaveBeenCalledWith({
      channel_auto_ack_enabled: true,
      channel_auto_ack_template: SAVED.channel_auto_ack_template,
    })
  })

  it('shows a load error instead of default values when the read fails', async () => {
    branding.admin.mockRejectedValue(new Error('Request failed with status code 500'))
    await mountForm()

    expect(wrapper!.text()).toContain('admin-branding-error-load')
    expect(wrapper!.find('form').exists()).toBe(false)
  })
})
