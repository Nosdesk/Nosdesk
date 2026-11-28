import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'

// The tickets list as it mounts, with the sync pool, the session and the
// saved views faked and the row renderers stubbed to list the ids they're
// given. What runs for real is the list's own wiring: the view resolved
// from the URL, its filter, the fall-through and the banner.

const ME = 'owner-uuid'

const pool = vi.hoisted(() => ({ tickets: [] as unknown[] }))

vi.mock('@/sync/useWorkspaceGroup', async () => {
  const { ref } = await import('vue')
  return { useWorkspaceGroupSubscription: () => ({ ready: ref(true), populated: ref(true) }) }
})
vi.mock('@/sync/stores/tickets', async () => {
  const { computed, ref } = await import('vue')
  return {
    useSyncTicketsStore: () => ({
      all: () => ref(pool.tickets),
      byId: () => computed(() => null),
      patchPlan: async () => {},
      bulkMoveToWorkflowState: async () => {},
      bulkPatchKanbanFields: async () => {},
    }),
  }
})
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ user: { uuid: 'owner-uuid' }, isAdmin: true }),
}))
vi.mock('@/stores/savedViews', async () => {
  const { ref } = await import('vue')
  return {
    useSavedViewsStore: () => ({
      viewsForProject: () => ref([]),
      ensureLoaded: async () => {},
      create: async () => null,
      update: async () => null,
      deleteView: async () => false,
    }),
  }
})
vi.mock('@/composables/useKnowledgeGaps', () => ({
  useFlagTicketMutation: () => ({ mutateAsync: async () => {} }),
}))

// Stand-ins for the renderers. A row list renders the ids of the cards it's
// handed; anything else renders an empty div.
const stubs = vi.hoisted(() => ({
  stub: async (name: string) => {
    const { defineComponent, h } = await import('vue')
    return { default: defineComponent({ name, render: () => h('div') }) }
  },
  rows: async (name: string) => {
    const { defineComponent, h } = await import('vue')
    return {
      default: defineComponent({
        name,
        props: { cards: { type: Array, default: () => [] } },
        render() {
          const cards = this.cards as Array<{ id: number }>
          return h('ul', { 'data-rows': name }, cards.map((c) => h('li', String(c.id))))
        },
      }),
    }
  },
}))
vi.mock('@/components/views/TicketsHeader.vue', () => stubs.stub('TicketsHeader'))
vi.mock('@/components/views/TicketsTable.vue', () => stubs.rows('TicketsTable'))
vi.mock('@/components/views/TicketsCardList.vue', () => stubs.rows('TicketsCardList'))
vi.mock('@/components/common/PullToRefresh.vue', () => stubs.stub('PullToRefresh'))
vi.mock('@/components/views/TicketPreviewPane.vue', () => stubs.stub('TicketPreviewPane'))
vi.mock('@/components/views/SavedViewEditorModal.vue', () => stubs.stub('SavedViewEditorModal'))
vi.mock('@/components/views/SaveViewModal.vue', () => stubs.stub('SaveViewModal'))
vi.mock('@/components/views/TicketsBulkBar.vue', () => stubs.stub('TicketsBulkBar'))
vi.mock('@/components/views/FirstTicketsState.vue', () => stubs.stub('FirstTicketsState'))
vi.mock('@/components/common/ContextMenu.vue', () => stubs.stub('ContextMenu'))
vi.mock('@/components/UserSelectionModal.vue', () => stubs.stub('UserSelectionModal'))
vi.mock('@/components/ticketComponents/MergeTicketsDialog.vue', () => stubs.stub('MergeTicketsDialog'))
vi.mock('./calendar/CalendarView.vue', () => stubs.stub('CalendarView'))
vi.mock('./agenda/AgendaView.vue', () => stubs.stub('AgendaView'))

import TicketsListView from './TicketsListView.vue'

let nextId = 1
function ticket(category: WorkflowStateCategory, assignee_uuid: string | null) {
  const id = nextId++
  const closed = category === 'done' || category === 'cancelled' || category === 'merged'
  return {
    id,
    number: id,
    title: `Ticket ${id}`,
    workflow_state: { id, name: category, category, color: 'gray' },
    priority: 'none',
    assignee_uuid,
    requester_uuid: null,
    due_date: null,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    last_activity_at: '2026-11-01T00:00:00Z',
    closed_at: closed ? '2026-11-02T00:00:00Z' : null,
  }
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  pool.tickets = []
})

async function mountAt(view: string): Promise<VueWrapper> {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/tickets', name: 'tickets', component: { render: () => null } }],
  })
  await router.push({ path: '/tickets', query: { view } })
  await router.isReady()
  wrapper = mountWithProviders(TicketsListView, {}, {}, [router])
  await flushPromises()
  return wrapper
}

const shownIds = (w: VueWrapper) =>
  w.findAll('[data-rows] li').map((li) => Number(li.text())).sort((a, b) => a - b)

describe('the tickets list with only closed tickets assigned to you', () => {
  it('shows the open, unassigned tickets under My Open, with the banner', async () => {
    const closed = [
      ...Array.from({ length: 12 }, () => ticket('done', ME)),
      ticket('cancelled', ME),
      ticket('merged', ME),
    ]
    const waiting = Array.from({ length: 16 }, () => ticket('triage', null))
    pool.tickets = [...closed, ...waiting]

    const w = await mountAt('my-open')
    expect(shownIds(w)).toEqual(waiting.map((t) => t.id))
    expect(w.text()).toContain('ticket-list-empty-no-assigned-message')
    expect(w.text()).toContain('ticket-list-empty-showing-all-active')
  })

  it('keeps to your open ticket, with no banner, once one is assigned', async () => {
    const mine = ticket('active', ME)
    pool.tickets = [ticket('done', ME), ticket('triage', null), mine]

    const w = await mountAt('my-open')
    expect(shownIds(w)).toEqual([mine.id])
    expect(w.text()).not.toContain('ticket-list-empty-no-assigned-message')
  })
})
