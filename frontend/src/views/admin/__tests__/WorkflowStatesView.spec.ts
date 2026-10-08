import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { WorkflowState } from '@nosdesk/core/types/workflow'

function state(id: number, name: string, category: WorkflowState['category'], archived = false): WorkflowState {
  return {
    id,
    name,
    category,
    color: 'blue',
    position: 0,
    is_default: false,
    archived_at: archived ? '2026-10-01T00:00:00Z' : null,
    created_at: '2026-01-01T00:00:00Z',
    created_by: null,
    pauses_sla: false,
  }
}

const OPEN = state(1, 'In Progress', 'active')
const WAITING = state(9, 'Waiting on vendor', 'active', true)

const service = vi.hoisted(() => ({
  listArchived: vi.fn(),
  restore: vi.fn(),
  update: vi.fn(),
  create: vi.fn(),
  archive: vi.fn(),
  list: vi.fn(),
}))
vi.mock('@nosdesk/core/services/workflowStatesService', () => ({ workflowStatesService: service }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({
    load: async () => [OPEN],
    states: [OPEN],
    byCategory: { triage: [], backlog: [], active: [OPEN], in_review: [], done: [], cancelled: [], merged: [] },
  }),
}))

import WorkflowStatesView from '@/views/admin/WorkflowStatesView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('WorkflowStatesView', () => {
  it('lists archived states and restores one', async () => {
    service.listArchived.mockResolvedValueOnce([WAITING]).mockResolvedValueOnce([])
    service.restore.mockResolvedValue({ ...WAITING, archived_at: null })
    wrapper = mountWithProviders(WorkflowStatesView)
    await flushPromises()

    const archived = wrapper.get('section[aria-label="admin-workflow-states-archived-heading"]')
    expect(archived.text()).toContain('Waiting on vendor')
    await archived.get('button[aria-label="admin-workflow-states-restore-label"]').trigger('click')
    await flushPromises()

    expect(service.restore).toHaveBeenCalledWith(9)
    expect(wrapper.find('section[aria-label="admin-workflow-states-archived-heading"]').exists()).toBe(false)
  })

  it('shows no archived section when nothing is archived', async () => {
    service.listArchived.mockResolvedValue([])
    wrapper = mountWithProviders(WorkflowStatesView)
    await flushPromises()

    expect(wrapper.find('section[aria-label="admin-workflow-states-archived-heading"]').exists()).toBe(false)
  })
})
