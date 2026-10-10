import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, nextTick } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The bulk "Assign" dialog offers the people the single-ticket assignee
// picker offers. The server refuses anyone else (`invalid_assignee`), so a
// requester in the list is a pick that silently does nothing.

const USERS = [
  { uuid: 'u-admin', name: 'Ada Admin', email: 'ada@example.test', platform_role: 'user', workspace_role: 'admin' },
  { uuid: 'u-agent', name: 'Grace Agent', email: 'grace@example.test', platform_role: 'user', workspace_role: 'agent' },
  { uuid: 'u-req', name: 'Rita Requester', email: 'rita@example.test', platform_role: 'user', workspace_role: 'member' },
  { uuid: 'u-plat', name: 'Paul Platform', email: 'paul@example.test', platform_role: 'platform_admin', workspace_role: null },
]

const server = vi.hoisted(() => ({ honoursRoleFilter: true, roles: [] as Array<string | undefined> }))
vi.mock('@/services/userService', () => ({
  default: {
    // As `/users/paginated` does: `admin,technician` is a platform admin or
    // a workspace owner, admin or agent.
    getPaginatedUsers: async ({ role }: { role?: string }) => {
      server.roles.push(role)
      const staff = (u: (typeof USERS)[number]) =>
        u.platform_role === 'platform_admin' || ['owner', 'admin', 'agent'].includes(u.workspace_role ?? '')
      const data = server.honoursRoleFilter && role === 'admin,technician' ? USERS.filter(staff) : USERS
      return { data, total: data.length, page: 1, page_size: 50, total_pages: 1 }
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
  server.honoursRoleFilter = true
  server.roles = []
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
  it('lists only people who can be assigned tickets, never a requester', async () => {
    const names = await openAssign()
    expect(server.roles).toEqual(['admin,technician'])
    expect(names).toEqual(['Ada Admin', 'Grace Agent', 'Paul Platform'])
  })

  it('still drops a requester the server returned', async () => {
    server.honoursRoleFilter = false
    const names = await openAssign()
    expect(names).not.toContain('Rita Requester')
    expect(names).toHaveLength(3)
  })
})
