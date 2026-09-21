import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick, ref } from 'vue'

// Recent tickets and ticket drafts persist to localStorage. Both are
// per-workspace lists on a single-origin instance, so their keys carry the
// workspace and a switch never hydrates the previous workspace's entries.

const slug = ref<string | null>('mercury')
vi.mock('@/services/activeWorkspace', () => ({
  activeWorkspaceSlugRef: slug,
  activeWorkspaceSlug: () => slug.value,
  setActiveWorkspaceSlug: (s: string | null) => (slug.value = s),
}))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ user: { uuid: 'user-1' }, isAuthenticated: true }),
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({
  default: { getRecentTickets: vi.fn(async () => []) },
}))

// Installs the in-memory localStorage shim the other specs rely on.
import '@/test/mountWithProviders'
import { configureStorage } from '@nosdesk/core/storage'
import { useTicketDraftsStore } from '@nosdesk/core/stores/ticketDrafts'

beforeEach(() => {
  setActivePinia(createPinia())
  localStorage.clear()
  // The core stores write through their own KV facade; point it at the
  // same localStorage so the keys can be asserted.
  configureStorage(localStorage)
  slug.value = 'mercury'
})

describe('ticket drafts across workspaces', () => {
  it('keeps each workspace\'s drafts under its own key', async () => {
    const drafts = useTicketDraftsStore()
    drafts.setScope('mercury')
    drafts.setDraft(7, { content: 'Mercury reply', isInternal: false })
    // Switch: the mercury draft is parked, ticket 7 on venus is empty.
    drafts.setScope('venus')
    expect(drafts.getDraft(7).content).toBe('')
    drafts.setDraft(7, { content: 'Venus note', isInternal: true })
    drafts.setScope('mercury')
    expect(drafts.getDraft(7).content).toBe('Mercury reply')
    drafts.setScope('venus')
    expect(drafts.getDraft(7).content).toBe('Venus note')
    expect(localStorage.getItem('nosdesk:ticket-drafts:mercury')).toContain('Mercury reply')
    await nextTick()
  })
})

describe('recent tickets across workspaces', () => {
  it('hydrates from a per-workspace key, not the account key', async () => {
    localStorage.setItem(
      'nosdesk:recent-tickets:user-1:mercury',
      JSON.stringify([{ id: 1, title: 'Mercury ticket' }]),
    )
    localStorage.setItem(
      'nosdesk:recent-tickets:user-1',
      JSON.stringify([{ id: 9, title: 'Account-wide leftover' }]),
    )
    const { useRecentTicketsStore } = await import('@/stores/recentTickets')
    const store = useRecentTicketsStore()
    await nextTick()
    expect(store.recentTickets.map((t) => t.title)).toEqual(['Mercury ticket'])
    // venus has nothing stored: its list starts empty rather than mercury's.
    slug.value = 'venus'
    await nextTick()
    expect(store.recentTickets.map((t) => t.title)).toEqual([])
  })
})
