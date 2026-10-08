import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import type { ServerEmailConfig } from '@nosdesk/core/services/workspaceEmailService'
import enUS from '../../../../../../i18n/locales/en-US/main.ftl?raw'

vi.mock('@nosdesk/core/services/workspaceEmailService', () => ({
  default: { setMode: vi.fn() },
}))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: vi.fn() }),
}))

import ServerDefaultPanel from '../ServerDefaultPanel.vue'

function render(config: ServerEmailConfig): string {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const wrapper = mount(ServerDefaultPanel, {
    props: { config, active: true },
    global: { plugins: [createFluentVue({ bundles: [bundle] })] },
  })
  const text = wrapper.text()
  wrapper.unmount()
  return text
}

const relay: ServerEmailConfig = {
  managed: false,
  provider: 'smtp',
  smtp_host: 'relay.example.com',
  smtp_port: 25,
  from_name: 'Help',
  from_email: 'help@example.com',
  enabled: true,
  is_configured: true,
  smtp_password_configured: false,
}

describe('ServerDefaultPanel', () => {
  it('shows a relay without credentials as signing in with none', () => {
    const text = render(relay)
    expect(text).toContain('No authentication')
    expect(text).toContain('relay.example.com:25')
  })

  it('shows a relay with credentials as signing in with them', () => {
    expect(
      render({ ...relay, smtp_password_configured: true, smtp_signs_in: true }),
    ).toContain('Username and password')
  })

  it('says credentials on a plaintext connection go unused', () => {
    expect(
      render({ ...relay, smtp_password_configured: true, smtp_signs_in: false }),
    ).toContain('Credentials ignored on a plaintext connection')
  })

  it('names what is wrong with settings that cannot be used', () => {
    const text = render({
      from_name: '',
      from_email: '',
      enabled: false,
      is_configured: false,
      error: "SMTP_USERNAME is set but SMTP_PASSWORD isn't",
    })
    expect(text).toContain(
      "The server's SMTP settings can't be used: SMTP_USERNAME is set but SMTP_PASSWORD isn't",
    )
  })
})
