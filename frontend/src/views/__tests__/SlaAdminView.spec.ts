import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('@nosdesk/core/services/slaService', () => ({
  slaService: {
    listPolicies: async () => [],
    listCalendars: async () => [],
    getPolicyMatchCounts: async () => ({}),
  },
}))
vi.mock('@nosdesk/core/services/categoryService', () => ({
  categoryService: { getCategories: async () => [] },
}))
vi.mock('@nosdesk/core/services/groupService', () => ({ groupService: { getGroups: async () => [] } }))
// The dropdown, reduced to the values it offers.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    props: ['options', 'modelValue'],
    template: `<div data-test="dropdown" :data-values="(options || []).map((o) => o.value).join(',')"></div>`,
  },
}))

import SlaAdminView from '@/views/SlaAdminView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

describe('SlaAdminView policy priority', () => {
  it('lets a policy target any of the five priorities', async () => {
    wrapper = mountWithProviders(SlaAdminView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const open = Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) =>
      b.textContent?.includes('admin-sla-new-policy-button'),
    )
    open!.click()
    await flushPromises()
    const offered = Array.from(document.body.querySelectorAll('[data-test="dropdown"]'))
      .map((d) => d.getAttribute('data-values'))
      .filter((v) => v?.includes('medium'))
    // '' is "any priority", saved as no filter.
    expect(offered).toEqual([',urgent,high,medium,low,none'])
  })
})
