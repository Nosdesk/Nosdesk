import { afterEach, describe, expect, it } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

import SectionCard from '@/components/common/SectionCard.vue'

let wrapper: VueWrapper | null = null

function mount(props: Record<string, unknown>) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:any(.*)*', component: { template: '<div />' } }],
  })
  wrapper = mountWithProviders(SectionCard, props, { title: () => 'Title' }, [router])
  return wrapper
}

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('SectionCard action link', () => {
  it('ends in an arrow screen readers skip', () => {
    mount({ actionTo: '/all', actionLabel: 'View all' })
    const link = wrapper!.find('a')
    expect(link.text()).toBe('View all →')
    expect(link.find('[aria-hidden="true"]').text()).toBe('→')
  })

  it('can leave the arrow off', () => {
    mount({ actionTo: '/matches', actionLabel: 'Manage', actionArrow: false })
    expect(wrapper!.find('a').text()).toBe('Manage')
  })
})
