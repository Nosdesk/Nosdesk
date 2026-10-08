import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { createPinia, setActivePinia } from 'pinia'

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
const toast = { error: vi.fn(), warning: vi.fn() }
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
vi.mock('@/i18n', () => ({ translate: (key: string) => key }))

import { useTicketDetail } from '../ticketDetail'
import { noteServerEcho } from '@/sync/optimisticCreates'
import { resendClientId, useTicketDraftsStore } from '@nosdesk/core/stores/ticketDrafts'
import { useTicketUiStore } from '@nosdesk/core/stores/ticketUi'

beforeAll(() => {
  // jsdom has no object URLs; the optimistic rows ask for one per file.
  URL.createObjectURL = vi.fn(() => 'blob:preview')
  URL.revokeObjectURL = vi.fn()
})
beforeEach(() => setActivePinia(createPinia()))
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

describe('a resend answered with the saved reply', () => {
  it('shows the reply as it was saved, not as the toggle stands now', async () => {
    const pool = await import('@nosdesk/core/sync/pool')
    addCommentToTicket.mockResolvedValueOnce({
      id: 130,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: '<p>Here is the log</p>',
      is_internal: false,
      created_at: '2026-10-06T01:53:25Z',
      attachments: [],
    })
    const detail = useTicketDetail(101)

    await detail.addComment({ ...reply(), files: [], is_internal: true, client_id: 'sent-before' })

    expect(pool.upsert).toHaveBeenCalledWith(
      'comment',
      130,
      expect.objectContaining({ id: 130, is_internal: false }),
    )
  })
})

// The composer clears as the reply goes out. A reply that doesn't make it
// comes back to the composer whole: its text, its files and whether it was
// an internal note, since restoring only the text would retry a note in public.
describe('a reply that fails to send', () => {
  const file = new File(['x'], 'attach.txt', { type: 'text/plain' })
  const upload = { data: [{ id: 7, url: '/uploads/temp/attach.txt', name: 'attach.txt' }] }

  it('goes back to the composer with its files', async () => {
    post.mockResolvedValueOnce(upload)
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment({ content: '<p>Here is the log</p>', user_uuid: 'agent-uuid', files: [file] })

    expect(useTicketDraftsStore().getDraft(101)).toMatchObject({
      content: '<p>Here is the log</p>',
      isInternal: false,
    })
    expect(useTicketUiStore().getAttachments(101)).toEqual([file])
    expect(toast.error).toHaveBeenCalledWith('ticket-comments-send-failed')
  })

  it('stays an internal note, and a retry is the same reply', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment({
      content: '<p>Vendor says Tuesday</p>',
      user_uuid: 'agent-uuid',
      files: [],
      is_internal: true,
    })

    const draft = useTicketDraftsStore().getDraft(101)
    expect(draft).toMatchObject({ content: '<p>Vendor says Tuesday</p>', isInternal: true })
    const sentAs = addCommentToTicket.mock.calls[0][4]
    expect(resendClientId(draft, [])).toBe(sentAs)

    addCommentToTicket.mockResolvedValueOnce({
      id: 127,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: draft.content,
      created_at: '2026-10-08T01:00:00Z',
      attachments: [],
    })
    await detail.addComment({
      content: draft.content,
      user_uuid: 'agent-uuid',
      files: [],
      is_internal: draft.isInternal,
      client_id: resendClientId(draft, []),
    })
    expect(addCommentToTicket.mock.calls[1][4]).toBe(sentAs)
  })

  it('goes after anything typed since', async () => {
    let fail!: (e: unknown) => void
    addCommentToTicket.mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    const detail = useTicketDetail(101)
    const drafts = useTicketDraftsStore()

    const sending = detail.addComment({ content: '<p>First</p>', user_uuid: 'agent-uuid', files: [] })
    drafts.setDraft(101, { content: '<p>Second</p>', isInternal: false })
    fail(new Error('network down'))
    await sending

    expect(drafts.getDraft(101).content).toBe('<p>First</p><p>Second</p>')
    // Not the failed reply alone any more, so a send is a new reply.
    expect(resendClientId(drafts.getDraft(101), [])).toBeUndefined()
  })

  it('comes back public when Internal was switched on in the emptied composer', async () => {
    let fail!: (e: unknown) => void
    addCommentToTicket.mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    const detail = useTicketDetail(101)
    const drafts = useTicketDraftsStore()

    const sending = detail.addComment({ content: '<p>Public answer</p>', user_uuid: 'agent-uuid', files: [] })
    drafts.setDraft(101, { content: '', isInternal: true })
    fail(new Error('network down'))
    await sending

    expect(drafts.getDraft(101)).toMatchObject({ content: '<p>Public answer</p>', isInternal: false })
  })

  it('is a new reply once it is edited or gains a file', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)
    await detail.addComment({ content: '<p>Here is the log</p>', user_uuid: 'agent-uuid', files: [] })
    const draft = useTicketDraftsStore().getDraft(101)
    const sentAs = addCommentToTicket.mock.calls[0][4]

    // The editor may write the same text back with different markup.
    expect(resendClientId({ ...draft, content: '<p>Here is the log</p><p></p>' }, [])).toBe(sentAs)
    expect(resendClientId({ ...draft, content: '<p>Here is the new log</p>' }, [])).toBeUndefined()
    expect(resendClientId(draft, [new File(['y'], 'more.txt')])).toBeUndefined()
  })

  it('is not put into another workspace, and says so', async () => {
    const drafts = useTicketDraftsStore()
    drafts.setScope('acme')
    let fail!: (e: unknown) => void
    addCommentToTicket.mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    const detail = useTicketDetail(101)

    const sending = detail.addComment({ content: '<p>Acme reply</p>', user_uuid: 'agent-uuid', files: [] })
    // The switch parks this workspace's drafts before the request is refused.
    drafts.setScope(null)
    fail(new Error('workspace changed'))
    await sending

    expect(drafts.getDraft(101).content).toBe('')
    drafts.setScope('acme')
    expect(drafts.getDraft(101).content).toBe('')
    expect(toast.error).toHaveBeenCalledWith('ticket-comments-send-failed-not-kept')
    drafts.setScope(null)
  })

  it('is not restored once the server has it', async () => {
    post.mockResolvedValueOnce(upload)
    addCommentToTicket.mockImplementationOnce((...args: unknown[]) => {
      // The comment was created: its sync echo arrived, then the response was lost.
      noteServerEcho(args[4] as string, 555)
      return Promise.reject(new Error('network down'))
    })
    const detail = useTicketDetail(101)

    await detail.addComment({ content: '<p>Here is the log</p>', user_uuid: 'agent-uuid', files: [file] })

    expect(useTicketDraftsStore().getDraft(101).content).toBe('')
    expect(useTicketUiStore().getAttachments(101)).toEqual([])
    expect(toast.error).not.toHaveBeenCalled()
  })
})

describe('a reply that lost files on the way', () => {
  it('says so', async () => {
    post.mockResolvedValueOnce({ data: [{ id: 7, url: '/uploads/temp/a.txt', name: 'a.txt' }] })
    addCommentToTicket.mockResolvedValueOnce({
      id: 128,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: '<p>Two files</p>',
      created_at: '2026-10-08T01:00:00Z',
      attachments: [{ id: 7, url: '/uploads/a.txt', name: 'a.txt' }],
    })
    const detail = useTicketDetail(101)

    await detail.addComment({
      content: '<p>Two files</p>',
      user_uuid: 'agent-uuid',
      files: [new File(['a'], 'a.txt'), new File(['b'], 'b.txt')],
    })

    expect(toast.warning).toHaveBeenCalledWith('ticket-comments-attachments-missing')
  })
})
