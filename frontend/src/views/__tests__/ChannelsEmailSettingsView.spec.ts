import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('vue-router', () => ({
  RouterLink: { template: '<a><slot /></a>' },
  useRoute: () => ({ name: 'admin-channels-email', params: {} }),
  useRouter: () => ({ push: vi.fn() }),
}))
// The channel the list returns, if any.
const saved = vi.hoisted(() => ({ channel: null as Record<string, unknown> | null }))
const service = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
}))
vi.mock('@nosdesk/core/services/channelsService', () => ({
  channelsService: {
    list: async () => (saved.channel ? [saved.channel] : []),
    create: service.create,
    update: service.update,
  },
}))
vi.mock('@nosdesk/core/apiClient', () => ({
  default: { get: async () => ({ data: { environment: 'production' } }) },
}))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  isHostedDeployment: () => false,
  isInboundForwardingEnabled: () => false,
}))
// The date picker, reduced to a text field over its value.
vi.mock('@/components/common/DatePicker.vue', () => ({
  default: {
    props: ['modelValue', 'label', 'description', 'max'],
    emits: ['update:modelValue'],
    template: `<label data-test="backlog-since" :data-max="max">{{ label }} {{ description }}
      <input :value="modelValue" @input="$emit('update:modelValue', $event.target.value)" /></label>`,
  },
}))

import ChannelsEmailSettingsView from '@/views/ChannelsEmailSettingsView.vue'

const CONFIG = {
  host: 'imap.example.com',
  port: 993,
  username: 'support@example.com',
  mailbox: 'INBOX',
  use_tls: true,
  reply_domain: 'example.com',
  insecure_skip_cert_verify: false,
}

function channel(config: Record<string, unknown>) {
  return {
    id: 4,
    provider: 'email_imap',
    name: 'Support',
    enabled: true,
    config,
    runtime_state: {},
    has_credential: true,
  }
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  saved.channel = null
  service.create.mockReset()
  service.update.mockReset()
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function mountView() {
  wrapper = mountWithProviders(ChannelsEmailSettingsView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

async function type(placeholder: string, value: string) {
  await wrapper!.find(`input[placeholder="${placeholder}"]`).setValue(value)
}

const backlogField = () => wrapper!.find('[data-test="backlog-since"]')

describe('ChannelsEmailSettingsView older mail', () => {
  it('explains that older mail comes in quietly and only up to today', async () => {
    await mountView()
    expect(backlogField().text()).toContain('admin-channels-email-field-backlog-since-label')
    expect(backlogField().text()).toContain('admin-channels-email-field-backlog-since-hint')
    expect(backlogField().attributes('data-max')).toMatch(/^\d{4}-\d{2}-\d{2}$/)
  })

  async function fillNewChannel() {
    await mountView()
    await type('admin-channels-email-field-name-placeholder', 'Support')
    await type('admin-channels-email-field-host-placeholder', CONFIG.host)
    await type('admin-channels-email-field-username-placeholder', CONFIG.username)
    await type('admin-channels-email-field-reply-domain-placeholder', CONFIG.reply_domain)
  }

  it('leaves the date out of a new channel when empty, so it takes only new mail', async () => {
    service.create.mockResolvedValue(channel(CONFIG))
    await fillNewChannel()
    await wrapper!.find('form').trigger('submit')
    await flushPromises()
    expect(service.create).toHaveBeenCalledOnce()
    expect(service.create.mock.calls[0][0].config).not.toHaveProperty('backlog_since')
  })

  it('sends the date a new channel brings older mail from', async () => {
    service.create.mockResolvedValue(channel({ ...CONFIG, backlog_since: '2026-01-01' }))
    await fillNewChannel()
    await backlogField().find('input').setValue('2026-01-01')
    await wrapper!.find('form').trigger('submit')
    await flushPromises()
    expect(service.create.mock.calls[0][0].config.backlog_since).toBe('2026-01-01')
  })

  it('keeps a saved date through an edit, and clearing it removes it', async () => {
    saved.channel = channel({ ...CONFIG, backlog_since: '2025-06-01' })
    service.update.mockImplementation(async (_id: number, body: { config: object }) =>
      channel(body.config as Record<string, unknown>),
    )
    await mountView()
    expect((backlogField().find('input').element as HTMLInputElement).value).toBe('2025-06-01')

    await wrapper!.find('form').trigger('submit')
    await flushPromises()
    expect(service.update.mock.calls[0][1].config.backlog_since).toBe('2025-06-01')

    await backlogField().find('input').setValue('')
    await wrapper!.find('form').trigger('submit')
    await flushPromises()
    expect(service.update.mock.calls[1][1].config).not.toHaveProperty('backlog_since')
  })
})
