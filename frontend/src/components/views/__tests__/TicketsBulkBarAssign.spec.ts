import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, nextTick } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The bulk "Assign" dialog offers the people the single-ticket assignee
// picker offers. The server refuses anyone else (`invalid_assignee`), so a
// requester in the list is a pick that silently does nothing.

// More requesters than a first page holds, all sorting before the staff.
const REQUESTERS = Array.from({ length: 60 }, (_, i) => ({
  uuid: `u-req-${i}`,
  name: `Aa Requester ${String(i).padStart(2, '0')}`,
  email: `req${i}@example.test`,
  platform_role: 'user',
  workspace_role: 'member',
}))
const STAFF = [
  { uuid: 'u-admin', name: 'Zz Admin', email: 'admin@example.test', platform_role: 'user', workspace_role: 'admin' },
  { uuid: 'u-agent', name: 'Zz Agent', email: 'agent@example.test', platform_role: 'user', workspace_role: 'agent' },
]
const USERS = [...REQUESTERS, ...STAFF]

const server = vi.hoisted(() => ({ params: [] as Array<Record<string, unknown>> }))
vi.mock('@/services/userService', () => ({
  default: {
    // As `/users/paginated` does: `assignable=true` filters to who can be
    // assigned, then the page is cut from the sorted result.
    getPaginatedUsers: async (params: { assignable?: boolean; pageSize?: number; page?: number }) => {
      server.params.push(params)
      const pool = params.assignable ? STAFF : USERS
      const size = params.pageSize ?? 25
      const start = ((params.page ?? 1) - 1) * size
      const data = [...pool].sort((a, b) => a.name.localeCompare(b.name)).slice(start, start + size)
      return { data, total: pool.length, page: params.page ?? 1, page_size: size, total_pages: Math.ceil(pool.length / size) }
    },
  },
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ byCategory: {}, findById: () => undefined }),
}))
vi.mock('@/sync/stores/tickets', () => ({
  useSyncTicketsStore: () => ({ byId: () => computed(() => null) }),
}))
vi.mock('@/plugins/loader', () => ({ getSlotRegistrations: () => [] }))
vi.mock('@/plugins/usePluginModal', () => ({ openPluginModal: () => {} }))
vi.mock('@/components/ticketComponents/MergeTicketsDialog.vue', () => ({ default: { template: '<div />' } }))

import TicketsBulkBar from '@/components/views/TicketsBulkBar.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  server.params = []
})

async function openAssign(): Promise<string[]> {
  wrapper = mountWithProviders(TicketsBulkBar, { selectedIds: ['10', '11'] })
  await nextTick()
  const assign = wrapper.findAll('button').find((b) => b.text() === 'ticket-list-bulk-assign')!
  await assign.trigger('click')
  await flushPromises()
  await nextTick()
  return Array.from(document.body.querySelectorAll('[role="option"]')).map(
    (o) => o.querySelector('.flex-1 > div')?.textContent?.trim() ?? '',
  )
}

describe('the bulk assign dialog', () => {
  it('asks the server for the people who can be assigned, and lists only them', async () => {
    const names = await openAssign()
    expect(server.params.map((p) => p.assignable)).toEqual([true])
    expect(names).toEqual(['Zz Admin', 'Zz Agent'])
  })
})
