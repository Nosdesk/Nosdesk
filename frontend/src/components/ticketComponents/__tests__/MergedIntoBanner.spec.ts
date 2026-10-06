import { describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { formatDateTime } from '@nosdesk/core/utils/dateUtils'
import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

const names: Record<string, string> = { 'agent-uuid': 'NosBot Test' }
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({
    getUserHandle: (uuid: string) => ({
      user: computed(() => (names[uuid] ? { name: names[uuid] } : undefined)),
    }),
  }),
}))
// Ticket id 100 is #98; any other id is one the viewer can't see.
vi.mock('@/composables/useTicketNumberLookup', () => ({
  numberForTicketId: (id: number) => (id === 100 ? 98 : undefined),
}))

import MergedIntoBanner from '../MergedIntoBanner.vue'

const when = '2026-10-06T01:54:00.902137'

function render(props: { targetId: number; actor?: string; when?: string }): string {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:rest(.*)*', component: { render: () => null } }],
  })
  const wrapper = mount(MergedIntoBanner, {
    props,
    global: { plugins: [createFluentVue({ bundles: [bundle] }), router] },
  })
  const text = wrapper.text()
  wrapper.unmount()
  return text
}

describe('MergedIntoBanner', () => {
  it('names the person who merged it and when, with the ticket number', () => {
    const text = render({ targetId: 100, actor: 'agent-uuid', when })
    expect(text).toContain(`This ticket was merged into #98 by NosBot Test on ${formatDateTime(when)}.`)
    expect(text).not.toContain('agent-uuid')
    expect(text).not.toContain('2026-10-06T01:54')
  })

  it('leaves out "by" until the name is known', () => {
    const text = render({ targetId: 100, actor: 'unknown-uuid', when })
    expect(text).toContain(`This ticket was merged into #98 on ${formatDateTime(when)}.`)
    expect(text).not.toContain('unknown-uuid')
  })

  it('says "another ticket" when the destination is out of view', () => {
    const text = render({ targetId: 555, actor: 'agent-uuid', when })
    expect(text).toContain(`This ticket was merged into another ticket by NosBot Test on ${formatDateTime(when)}.`)
  })
})
