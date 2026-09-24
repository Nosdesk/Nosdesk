import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// A load started before a workspace switch must not fill the store the switch
// emptied, nor stop the new workspace's load from running.

const pending: Array<(v: unknown) => void> = []
vi.mock('@nosdesk/core/services/workflowStatesService', () => ({
  workflowStatesService: {
    list: () => new Promise((resolve) => pending.push(resolve)),
  },
}))

import { useWorkflowStatesStore } from '@nosdesk/core/stores/workflowStates'

const state = (id: number) => ({
  id,
  name: `S${id}`,
  category: 'backlog',
  color: '#000',
  position: id,
  is_default: false,
  archived_at: null,
})

beforeEach(() => {
  setActivePinia(createPinia())
  pending.length = 0
})

describe('workflow states store across a reset', () => {
  it('drops a load that finishes after reset and runs a fresh one', async () => {
    const store = useWorkflowStatesStore()
    const stale = store.load()
    store.reset()

    const fresh = store.load()
    expect(pending).toHaveLength(2)

    pending[1]([state(2)])
    await fresh
    pending[0]([state(1)])
    await stale

    expect(store.states.map((s) => s.id)).toEqual([2])
    expect(store.loaded).toBe(true)
  })
})
