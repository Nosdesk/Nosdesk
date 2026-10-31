import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ user: { uuid: 'me' } }),
}))
vi.mock('@nosdesk/core/services/watcherService', () => ({
  watcherService: {
    myState: vi.fn(async () => ({ notify_on_internal_notes: true })),
    updatePreferences: vi.fn(),
  },
}))

import TicketWatchersField from '../TicketWatchersField.vue'

function mountField(props: { watcherUuids: string[]; readonly?: boolean }) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return mount(TicketWatchersField, {
    props: { ticketId: 7, ...props },
    global: {
      plugins: [createFluentVue({ bundles: [bundle] })],
      stubs: {
        UserAvatar: true,
        ResponsiveMenu: true,
        UserPicker: { name: 'UserPicker', template: '<div class="user-picker" />', emits: ['pick'] },
      },
    },
  })
}

const toggle = (w: ReturnType<typeof mountField>) => w.find('button[aria-pressed]')

describe('TicketWatchersField on a merged ticket', () => {
  it('lets a watcher stop watching, without the preferences', async () => {
    const w = mountField({ watcherUuids: ['me'], readonly: true })
    expect(toggle(w).exists()).toBe(true)
    expect(w.find('button[aria-haspopup="dialog"]').exists()).toBe(false)
    await toggle(w).trigger('click')
    expect(w.emitted('toggle')).toHaveLength(1)
  })

  it("doesn't offer to start watching", () => {
    const w = mountField({ watcherUuids: [], readonly: true })
    expect(toggle(w).exists()).toBe(false)
  })

  it('offers both on any other ticket', () => {
    expect(toggle(mountField({ watcherUuids: [] })).exists()).toBe(true)
    expect(toggle(mountField({ watcherUuids: ['me'] })).exists()).toBe(true)
  })
})

const manage = (w: ReturnType<typeof mountField>) => w.find('button[aria-label="Add or remove watchers"]')

describe('TicketWatchersField add and remove', () => {
  it('adds a picked person once and removes a listed one', async () => {
    const w = mountField({ watcherUuids: ['me', 'colleague'] })
    await manage(w).trigger('click')
    const picker = w.findComponent({ name: 'UserPicker' })
    picker.vm.$emit('pick', { uuid: 'customer', name: 'Customer', email: '' })
    picker.vm.$emit('pick', { uuid: 'colleague', name: 'Colleague', email: '' })
    expect(w.emitted('set-watcher')).toEqual([['customer', true]])

    const removes = w.findAll('button[aria-label="Remove watcher"]')
    expect(removes).toHaveLength(2)
    await removes[1].trigger('click')
    expect(w.emitted('set-watcher')?.[1]).toEqual(['colleague', false])
  })

  it('is not offered on a merged ticket', () => {
    expect(manage(mountField({ watcherUuids: ['me'], readonly: true })).exists()).toBe(false)
  })
})
