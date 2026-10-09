import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const policies = vi.hoisted(() => ({ rows: [] as unknown[] }))
vi.mock('@nosdesk/core/services/slaService', () => ({
  slaService: {
    listPolicies: async () => policies.rows,
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
  policies.rows = []
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

describe('SlaAdminView targets', () => {
  it('shows targets as working hours and a missing calendar as the workspace default', async () => {
    policies.rows = [
      {
        id: 1,
        name: 'Standard',
        target_response_minutes: 240,
        target_resolution_minutes: 1440,
        working_calendar_id: null,
        priority_filter: null,
        category_id_filter: null,
        assignee_group_id_filter: null,
        is_default: true,
        no_sla: false,
        clock_start: 'activated',
      },
    ]
    wrapper = mountWithProviders(SlaAdminView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const row = Array.from(document.body.querySelectorAll('tr')).find((r) =>
      r.textContent?.includes('Standard'),
    )
    // 1440 working minutes is three 8-hour days, so "1d" misread it.
    expect(row?.textContent).toContain('24h')
    expect(row?.textContent).not.toContain('1d')
    expect(row?.textContent).toContain('admin-sla-calendar-workspace-default')
  })

  it('asks for targets of at least one minute', async () => {
    wrapper = mountWithProviders(SlaAdminView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const open = Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) =>
      b.textContent?.includes('admin-sla-new-policy-button'),
    )
    open!.click()
    await flushPromises()
    const mins = ['admin-sla-field-response', 'admin-sla-field-resolution'].map((key) => {
      const label = Array.from(document.body.querySelectorAll('label')).find((l) =>
        l.textContent?.includes(key),
      )
      const input = document.body.querySelector<HTMLInputElement>(
        `#${label!.getAttribute('for')} input, input#${label!.getAttribute('for')}`,
      )
      return input?.getAttribute('aria-valuemin')
    })
    expect(mins).toEqual(['1', '1'])
  })
})
