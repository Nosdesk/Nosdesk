import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
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
const addCommentToTicket = vi.fn()
vi.mock('@nosdesk/core/services/ticketService', () => ({
  default: { addCommentToTicket: (...args: unknown[]) => addCommentToTicket(...args) },
  getCommentsByTicketId: vi.fn(),
}))
const post = vi.fn()
vi.mock('@nosdesk/core/apiClient', () => ({ default: { post: (...args: unknown[]) => post(...args) } }))
vi.mock('@nosdesk/core/services/projectService', () => ({ projectService: {} }))
vi.mock('@/services/attachmentPreviewCache', () => ({ stashPreview: vi.fn() }))
const toast = { error: vi.fn() }
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
vi.mock('@/i18n', () => ({ translate: (key: string) => key }))

import { useTicketDetail } from '../ticketDetail'

beforeAll(() => {
  // jsdom has no object URLs; the optimistic rows ask for one per file.
  URL.createObjectURL = vi.fn(() => 'blob:preview')
  URL.revokeObjectURL = vi.fn()
})
afterEach(() => vi.clearAllMocks())

/** Whether leaving the page right now would ask first. */
function leavingAsks(): boolean {
  const e = new Event('beforeunload', { cancelable: true })
  window.dispatchEvent(e)
  return e.defaultPrevented
}

function deferred<T>() {
  let resolve!: (v: T) => void
  let reject!: (e: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

const reply = () => ({
  content: '<p>Here is the log</p>',
  user_uuid: 'agent-uuid',
  files: [new File(['x'], 'attach.txt', { type: 'text/plain' })],
})

describe('a reply with a file', () => {
  it('asks before the page unloads until the reply is created', async () => {
    const upload = deferred<{ data: { id: number; url: string; name: string }[] }>()
    post.mockReturnValueOnce(upload.promise)
    addCommentToTicket.mockResolvedValueOnce({
      id: 126,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: '<p>Here is the log</p>',
      created_at: '2026-10-06T01:53:25Z',
      attachments: [],
    })
    const detail = useTicketDetail(101)

    expect(leavingAsks()).toBe(false)
    const sending = detail.addComment(reply())
    // The upload is still running, and the reply isn't created yet.
    expect(leavingAsks()).toBe(true)

    upload.resolve({ data: [{ id: 7, url: '/uploads/temp/attach.txt', name: 'attach.txt' }] })
    await sending
    expect(addCommentToTicket).toHaveBeenCalledWith(
      101,
      '<p>Here is the log</p>',
      [expect.objectContaining({ id: 7 })],
      false,
      expect.any(String),
    )
    expect(leavingAsks()).toBe(false)
  })

  it('stops asking and says so when the reply fails', async () => {
    post.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment(reply())

    expect(leavingAsks()).toBe(false)
    expect(toast.error).toHaveBeenCalledWith('ticket-comments-send-failed')
    expect(addCommentToTicket).not.toHaveBeenCalled()
  })
})
