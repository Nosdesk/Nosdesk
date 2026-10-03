import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const calls = { preview: 0, sent: 0 }
vi.mock('@nosdesk/core/services/brandingService', () => ({
  default: {
    getEmailPreview: async () => {
      calls.preview += 1
      return { light: `<p>light ${calls.preview}</p>`, dark: `<p>dark ${calls.preview}</p>` }
    },
  },
}))
vi.mock('@nosdesk/core/services/workspaceEmailService', () => ({
  default: {
    getServerConfig: async () => ({ managed: false }),
    sendTest: async () => {
      calls.sent += 1
      return { ok: true, to: 'admin@example.com', code: null, detail: null }
    },
  },
}))

import EmailBrandingPreview from '@/components/admin/branding/EmailBrandingPreview.vue'

let wrapper: VueWrapper | null = null
beforeEach(() => {
  calls.preview = 0
  calls.sent = 0
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function mountPreview(version = 'v1') {
  wrapper = mountWithProviders(EmailBrandingPreview, { version }, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}
const frame = () => wrapper!.get('iframe')
const hint = () => wrapper!.text().includes('admin-branding-email-dark-hint')

describe('EmailBrandingPreview', () => {
  it('shows the light paper first, in a frame that runs nothing', async () => {
    await mountPreview()
    expect(frame().attributes('srcdoc')).toBe('<p>light 1</p>')
    expect(frame().attributes('sandbox')).toBe('allow-same-origin')
    expect(hint()).toBe(false)
  })

  it('switches to the dark paper, with a note on which apps show it', async () => {
    await mountPreview()
    const dark = wrapper!.findAll('[role="radio"]').find((r) => r.text() === 'admin-branding-email-dark')
    await dark!.trigger('click')
    await flushPromises()
    expect(frame().attributes('srcdoc')).toBe('<p>dark 1</p>')
    expect(hint()).toBe(true)
  })

  it('draws again when the branding changes', async () => {
    await mountPreview('v1')
    await wrapper!.setProps({ version: 'v2' })
    await flushPromises()
    expect(calls.preview).toBe(2)
    expect(frame().attributes('srcdoc')).toBe('<p>light 2</p>')
  })

  it('sends the test email and shows the result', async () => {
    await mountPreview()
    await wrapper!.get('button:not([role="radio"])').trigger('click')
    await flushPromises()
    expect(calls.sent).toBe(1)
    expect(wrapper!.text()).toContain('email-test-ok-title')
  })
})
