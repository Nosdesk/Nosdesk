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
      stubs: { UserAvatar: true, ResponsiveMenu: true },
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
