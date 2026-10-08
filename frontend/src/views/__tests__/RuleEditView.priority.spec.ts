import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule } from '@nosdesk/core/types/rule'

const route = vi.hoisted(() => ({ name: 'admin-rules-edit' as string, params: { id: '4' } as Record<string, string> }))
const rules = vi.hoisted(() => ({ get: vi.fn(), create: vi.fn(), update: vi.fn(), transitionState: vi.fn() }))
vi.mock('vue-router', () => ({ useRoute: () => route, useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => ({ success: vi.fn(), error: vi.fn() }) }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], states: [] }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({ useTagsStore: () => ({ tags: [] }) }))
vi.mock('@nosdesk/core/services/groupService', () => ({ groupService: { getGroups: async () => [] } }))
vi.mock('@/components/ticketComponents/UserPicker.vue', () => ({
  default: { props: ['modelValue'], template: '<input data-test="user-picker" />' },
}))
// The dropdown, reduced to the values it offers.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    props: ['options', 'modelValue'],
    template: `<div data-test="dropdown" :data-values="(options || []).map((o) => o.value).join(',')"></div>`,
  },
}))

import RuleEditView from '@/views/RuleEditView.vue'

const rule: Rule = {
  id: 4,
  workspace_id: 1,
  name: 'Bump priority',
  description: null,
  trigger_kind: 'manual',
  trigger_config: {},
  conditions: [],
  actions: [{ kind: 'set_priority', config: { priority: 'high' } }],
  reads_set: [],
  writes_set: [],
  state: 'draft',
  priority: 100,
  last_fired_at: null,
  fire_count: 0,
  created_by: null,
  created_at: '2026-10-01T00:00:00Z',
  updated_at: '2026-10-01T00:00:00Z',
  archived_at: null,
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

describe('RuleEditView priority step', () => {
  it('offers every priority, none included', async () => {
    rules.get.mockResolvedValue(rule)
    wrapper = mountWithProviders(RuleEditView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    const offered = wrapper
      .findAll('[data-test="dropdown"]')
      .map((d) => d.attributes('data-values'))
      .filter((v) => v?.includes('medium'))
    expect(offered).toEqual(['urgent,high,medium,low,none'])
  })
})
