import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import IconButton from '@/components/common/IconButton.vue'
import type { Rule } from '@nosdesk/core/types/rule'
import type { WorkflowState } from '@nosdesk/core/types/workflow'

// Archiving keeps the record and can be undone, so its button shows the
// archive box, not the bin that means deleting.

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

const state: WorkflowState = {
  id: 9,
  name: 'Waiting on vendor',
  category: 'active',
  color: 'amber',
  position: 1,
  is_default: false,
  archived_at: null,
  created_at: '2026-10-01T00:00:00Z',
  created_by: null,
  pauses_sla: false,
}

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: { list: async () => [rule] } }))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: vi.fn(), error: vi.fn() }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({
    load: async () => [state],
    states: [state],
    byCategory: { triage: [], backlog: [], active: [state], in_review: [], done: [], cancelled: [], merged: [] },
  }),
}))

import SettingsRulesView from '@/views/SettingsRulesView.vue'
import WorkflowStatesView from '@/views/admin/WorkflowStatesView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

function iconFor(label: string): string | undefined {
  return wrapper!
    .findAllComponents(IconButton)
    .find((b) => b.props('label') === label)
    ?.props('icon')
}

describe('archive buttons', () => {
  it('show the archive icon on a rule', async () => {
    wrapper = mountWithProviders(SettingsRulesView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()
    expect(iconFor('admin-rules-action-archive-tooltip')).toBe('archive')
  })

  it('show the archive icon on a workflow state', async () => {
    wrapper = mountWithProviders(WorkflowStatesView)
    await flushPromises()
    expect(iconFor('admin-workflow-states-archive-title')).toBe('archive')
  })
})
