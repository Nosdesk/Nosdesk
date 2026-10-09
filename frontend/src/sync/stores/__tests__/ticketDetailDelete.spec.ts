import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'

const push = vi.fn()
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))
vi.mock('@nosdesk/core/sync/composables', () => ({
  useEntity: () => ref(null),
  useReference: () => ref(null),
  useAggregate: () => ref([]),
}))
vi.mock('@nosdesk/core/sync/pool', () => ({ upsert: vi.fn(), remove: vi.fn(), get: vi.fn() }))
vi.mock('@/sync/queue', () => ({ dispatchOptimistic: vi.fn() }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({ useWorkflowStatesStore: () => ({}) }))
vi.mock('@/stores/recentTickets', () => ({
  useRecentTicketsStore: () => ({ recordTicketView: vi.fn(), updateTicketData: vi.fn() }),
}))
const deleteTicket = vi.fn()
vi.mock('@nosdesk/core/services/ticketService', () => ({
  default: { deleteTicket: (...args: unknown[]) => deleteTicket(...args) },
  getCommentsByTicketId: vi.fn(),
}))
vi.mock('@nosdesk/core/services/projectService', () => ({ projectService: {} }))
const toast = { error: vi.fn(), warning: vi.fn(), removeToast: vi.fn() }
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
vi.mock('@/i18n', () => ({ translate: (key: string) => key }))

import { useTicketDetail } from '../ticketDetail'

beforeEach(() => setActivePinia(createPinia()))
afterEach(() => vi.clearAllMocks())

describe('deleting a ticket', () => {
  it('says so when the server refuses, and stays on the ticket', async () => {
    deleteTicket.mockRejectedValueOnce(Object.assign(new Error('Forbidden'), { response: { status: 403 } }))
    await useTicketDetail(101).deleteTicket()

    expect(toast.error).toHaveBeenCalledWith('ticket-delete-failed')
    expect(push).not.toHaveBeenCalled()
  })

  it('goes back to the list once it is deleted', async () => {
    deleteTicket.mockResolvedValueOnce(undefined)
    await useTicketDetail(101).deleteTicket()

    expect(push).toHaveBeenCalledWith('/tickets')
    expect(toast.error).not.toHaveBeenCalled()
  })
})
