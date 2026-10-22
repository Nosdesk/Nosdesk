import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// A documentation page's revisions, as `/collaboration/docs/{id}/revisions`
// answers: the first one an import wrote, with its summary, then an edit.
const api = vi.hoisted(() => ({
  get: vi.fn(async () => ({
    data: [
      { id: 12, revision_number: 2, created_by: null, created_at: '2026-10-22T10:00:00Z', change_summary: null },
      {
        id: 11,
        revision_number: 1,
        created_by: null,
        created_at: '2026-10-22T09:00:00Z',
        change_summary: 'Imported from handbook.zip',
      },
    ],
  })),
}))
vi.mock('@nosdesk/core/apiClient', () => ({ default: api }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isTechnician: false }) }))
vi.mock('@/composables/useUsersDirectory', () => ({ useUsersDirectory: () => ({ getUserHandle: () => null }) }))

import RevisionList from '@/components/editor/RevisionList.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('RevisionList', () => {
  it("shows a documentation revision's summary on that revision only", async () => {
    wrapper = mountWithProviders(RevisionList, { type: 'documentation', documentId: 5 })
    await flushPromises()

    expect(api.get).toHaveBeenCalledWith('/collaboration/docs/5/revisions')
    const items = wrapper.findAll('.revision-item')
    expect(items).toHaveLength(2)
    expect(items[1].text()).toContain('v1')
    expect(items[1].text()).toContain('Imported from handbook.zip')
    expect(items[0].text()).not.toContain('Imported from')
  })
})
