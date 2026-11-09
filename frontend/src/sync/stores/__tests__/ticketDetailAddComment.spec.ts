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
const stashPreview = vi.fn()
vi.mock('@/services/attachmentPreviewCache', () => ({
  stashPreview: (...args: unknown[]) => stashPreview(...args),
}))
const toast = { error: vi.fn(() => 'failure-toast'), warning: vi.fn(), removeToast: vi.fn() }
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
const translate = vi.fn((key: string, _args?: unknown, _fallback?: string) => key)
vi.mock('@/i18n', () => ({
  translate: (key: string, args?: unknown, fallback?: string) => translate(key, args, fallback),
}))

import { useTicketDetail } from '../ticketDetail'
import { isEchoSuppressed, noteServerEcho } from '@/sync/optimisticCreates'
import { resendClientId, useTicketDraftsStore } from '@nosdesk/core/stores/ticketDrafts'
import { useTicketUiStore, type StagedAttachment } from '@nosdesk/core/stores/ticketUi'

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

/** A file staged on the composer and already uploaded as `id`. */
function staged(name: string, id: number): StagedAttachment {
  return {
    key: `staged-${name}-${id}`,
    file: new File(['x'], name, { type: 'text/plain' }),
    previewUrl: `blob:${name}`,
    status: 'uploaded',
    loaded: 1,
    total: 1,
    uploaded: { id, url: `/uploads/temp/${name}`, name },
    failure: null,
  }
}

const reply = () => ({
  content: '<p>Here is the log</p>',
  user_uuid: 'agent-uuid',
  attachments: [staged('attach.txt', 7)],
})

describe('a reply with a file', () => {
  it('posts the uploaded ids, uploading nothing more', async () => {
    const created = deferred<unknown>()
    addCommentToTicket.mockReturnValueOnce(created.promise)
    const detail = useTicketDetail(101)

    const sending = detail.addComment({ ...reply(), attachments: [staged('a.txt', 7), staged('b.png', 8)] })
    created.resolve({
      id: 126,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: '<p>Here is the log</p>',
      created_at: '2026-10-06T01:53:25Z',
      attachments: [],
    })
    await sending

    expect(post).not.toHaveBeenCalled()
    expect(addCommentToTicket).toHaveBeenCalledWith(
      101,
      '<p>Here is the log</p>',
      [
        { id: 7, url: '/uploads/temp/a.txt', name: 'a.txt' },
        { id: 8, url: '/uploads/temp/b.png', name: 'b.png' },
      ],
      false,
      expect.any(String),
    )
  })

  it('shows the files from their composer previews while it sends', async () => {
    const pool = await import('@nosdesk/core/sync/pool')
    const created = deferred<unknown>()
    addCommentToTicket.mockReturnValueOnce(created.promise)
    const detail = useTicketDetail(101)

    const sending = detail.addComment(reply())

    expect(pool.upsert).toHaveBeenCalledWith(
      'attachment',
      expect.any(Number),
      expect.objectContaining({ name: 'attach.txt', url: 'blob:attach.txt' }),
    )
    expect(URL.createObjectURL).not.toHaveBeenCalled()
    created.resolve(undefined)
    await sending
  })

  it('asks before the page unloads until the reply is created', async () => {
    const created = deferred<unknown>()
    addCommentToTicket.mockReturnValueOnce(created.promise)
    const detail = useTicketDetail(101)

    expect(leavingAsks()).toBe(false)
    const sending = detail.addComment(reply())
    expect(leavingAsks()).toBe(true)

    created.resolve({
      id: 126,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: '<p>Here is the log</p>',
      created_at: '2026-10-06T01:53:25Z',
      attachments: [],
    })
    await sending
    expect(leavingAsks()).toBe(false)
  })

  it('stops asking and says so when the reply fails', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment(reply())

    expect(leavingAsks()).toBe(false)
    expect(toast.error).toHaveBeenCalledWith('ticket-comments-send-failed')
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

    await detail.addComment({ ...reply(), attachments: [], is_internal: true, client_id: 'sent-before' })

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
  it('goes back to the composer with its files, still uploaded', async () => {
    const entry = staged('attach.txt', 7)
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment({ content: '<p>Here is the log</p>', user_uuid: 'agent-uuid', attachments: [entry] })

    expect(useTicketDraftsStore().getDraft(101)).toMatchObject({
      content: '<p>Here is the log</p>',
      isInternal: false,
    })
    const back = useTicketUiStore().getAttachments(101)
    expect(back).toHaveLength(1)
    expect(back[0]).toMatchObject({ status: 'uploaded', uploaded: { id: 7 }, previewUrl: 'blob:attach.txt' })
    // Its preview goes back with it, so it isn't revoked.
    expect(URL.revokeObjectURL).not.toHaveBeenCalled()
    expect(resendClientId(useTicketDraftsStore().getDraft(101), [entry.file])).toBe(
      addCommentToTicket.mock.calls[0][4],
    )
    expect(toast.error).toHaveBeenCalledWith('ticket-comments-send-failed')
  })

  it('stays an internal note, and a retry is the same reply', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment({
      content: '<p>Vendor says Tuesday</p>',
      user_uuid: 'agent-uuid',
      attachments: [],
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
      attachments: [],
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

    const sending = detail.addComment({ content: '<p>First</p>', user_uuid: 'agent-uuid', attachments: [] })
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

    const sending = detail.addComment({ content: '<p>Public answer</p>', user_uuid: 'agent-uuid', attachments: [] })
    drafts.setDraft(101, { content: '', isInternal: true })
    fail(new Error('network down'))
    await sending

    expect(drafts.getDraft(101)).toMatchObject({ content: '<p>Public answer</p>', isInternal: false })
  })

  it('is a new reply once it is edited or gains a file', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)
    await detail.addComment({ content: '<p>Here is the log</p>', user_uuid: 'agent-uuid', attachments: [] })
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

    const sending = detail.addComment({ content: '<p>Acme reply</p>', user_uuid: 'agent-uuid', attachments: [] })
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
    addCommentToTicket.mockImplementationOnce((...args: unknown[]) => {
      // The comment was created: its sync echo arrived, then the response was lost.
      noteServerEcho(args[4] as string, 555)
      return Promise.reject(new Error('network down'))
    })
    const detail = useTicketDetail(101)

    await detail.addComment(reply())

    expect(useTicketDraftsStore().getDraft(101).content).toBe('')
    expect(useTicketUiStore().getAttachments(101)).toEqual([])
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:attach.txt')
    expect(toast.error).not.toHaveBeenCalled()
  })
})

describe('a reply that lost files on the way', () => {
  it('says so', async () => {
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
      attachments: [staged('a.txt', 7), staged('b.txt', 8)],
    })

    expect(toast.warning).toHaveBeenCalledWith('ticket-comments-attachments-missing')
  })
})

// A failed reply goes back to the composer, so the next one sent from the
// ticket carries it; the failure toast (errors stay until closed) goes then.
describe('the failure toast', () => {
  it('closes when the reply is sent again', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)
    await detail.addComment({ content: '<p>Here is the log</p>', user_uuid: 'agent-uuid', attachments: [] })
    expect(toast.error).toHaveBeenCalledTimes(1)
    const draft = useTicketDraftsStore().getDraft(101)

    addCommentToTicket.mockResolvedValueOnce({
      id: 131,
      ticket_id: 101,
      user_uuid: 'agent-uuid',
      content: draft.content,
      created_at: '2026-10-08T01:00:00Z',
      attachments: [],
    })
    await detail.addComment({
      content: draft.content,
      user_uuid: 'agent-uuid',
      attachments: [],
      client_id: resendClientId(draft, []),
    })
    expect(toast.removeToast).toHaveBeenCalledWith('failure-toast')
  })

  it('names a file the server refused, and why', async () => {
    addCommentToTicket.mockRejectedValueOnce({
      response: {
        status: 400,
        data: { code: 'FILE_TYPE_NOT_ALLOWED', file: 'setup.bin', error: "File extension '.bin' is not allowed" },
      },
    })
    const detail = useTicketDetail(101)
    await detail.addComment(reply())
    expect(translate).toHaveBeenCalledWith(
      'ticket-comments-send-failed-file-type',
      { kind: 'reply', name: 'setup.bin' },
      expect.any(String),
    )

    addCommentToTicket.mockRejectedValueOnce({
      response: { status: 400, data: { code: 'INVALID_FILENAME', file: '..', error: 'Invalid filename' } },
    })
    await detail.addComment(reply())
    expect(translate).toHaveBeenCalledWith(
      'ticket-comments-send-failed-file-name',
      { kind: 'reply', name: '..' },
      expect.any(String),
    )
  })

  it('says the attachments are too large, by any of the ways it is told', async () => {
    const detail = useTicketDetail(101)
    for (const err of [
      { response: { status: 413, data: { code: 'FILE_TOO_LARGE', file: 'clip.mov' } } },
      { response: { status: 413, data: {} } },
      { code: 'ERR_UPLOAD_TOO_LARGE' },
    ]) {
      translate.mockClear()
      addCommentToTicket.mockRejectedValueOnce(err)
      await detail.addComment(reply())
      expect(translate).toHaveBeenCalledWith(
        'ticket-comments-send-failed-too-large',
        { kind: 'reply' },
        expect.any(String),
      )
    }
  })

  it('says the server could not be reached only when the connection failed', async () => {
    addCommentToTicket.mockRejectedValueOnce({ code: 'ERR_NETWORK', message: 'Network Error' })
    const detail = useTicketDetail(101)
    await detail.addComment(reply())
    expect(translate).toHaveBeenCalledWith(
      'ticket-comments-send-failed-offline',
      { kind: 'reply' },
      expect.any(String),
    )

    // A server error, or anything else, is just a failure to try again.
    for (const err of [{ response: { status: 500, data: { code: 'INTERNAL_ERROR' } } }, new Error('boom')]) {
      translate.mockClear()
      addCommentToTicket.mockRejectedValueOnce(err)
      await detail.addComment(reply())
      expect(translate).toHaveBeenCalledWith('ticket-comments-send-failed', { kind: 'reply' }, expect.any(String))
    }
  })

  it('says note for an internal note', async () => {
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)
    await detail.addComment({
      content: '<p>Vendor says Tuesday</p>',
      user_uuid: 'agent-uuid',
      attachments: [],
      is_internal: true,
    })
    expect(translate).toHaveBeenCalledWith(
      'ticket-comments-send-failed',
      { kind: 'note' },
      "Your note wasn't sent. Try again.",
    )
  })
})

// The Sending bubble is the temp row; once the request is answered it goes,
// whatever happens after.
describe('a sent reply', () => {
  const sent = {
    id: 132,
    ticket_id: 101,
    user_uuid: 'agent-uuid',
    content: '<p>Here is the log</p>',
    created_at: '2026-10-08T01:00:00Z',
    attachments: [{ id: 7, url: '/uploads/attach.txt', name: 'attach.txt' }],
  }

  it('leaves no Sending row and is not offered again when showing it fails', async () => {
    const pool = await import('@nosdesk/core/sync/pool')
    addCommentToTicket.mockResolvedValueOnce(sent)
    stashPreview.mockImplementationOnce(() => {
      throw new Error('preview cache full')
    })
    const detail = useTicketDetail(101)

    await detail.addComment(reply())

    const tempId = (pool.upsert as ReturnType<typeof vi.fn>).mock.calls.find(
      ([kind, id]) => kind === 'comment' && (id as number) < 0,
    )![1]
    expect(pool.remove).toHaveBeenCalledWith('comment', tempId)
    expect(pool.remove).toHaveBeenCalledWith('attachment', (tempId as number) - 1)
    expect(useTicketDraftsStore().getDraft(101).content).toBe('')
    expect(toast.error).not.toHaveBeenCalled()
    expect(leavingAsks()).toBe(false)
  })

  it('leaves no Sending row when the send fails', async () => {
    const pool = await import('@nosdesk/core/sync/pool')
    addCommentToTicket.mockRejectedValueOnce(new Error('network down'))
    const detail = useTicketDetail(101)

    await detail.addComment({ ...reply(), attachments: [] })

    const tempId = (pool.upsert as ReturnType<typeof vi.fn>).mock.calls.find(
      ([kind, id]) => kind === 'comment' && (id as number) < 0,
    )![1]
    expect(pool.remove).toHaveBeenCalledWith('comment', tempId)
  })
})

describe('a reply saved with an empty response', () => {
  it('shows through its server echo and is not offered again', async () => {
    const pool = await import('@nosdesk/core/sync/pool')
    addCommentToTicket.mockImplementationOnce((...args: unknown[]) => {
      // The echo arrives while the request is out; the 2xx carries no body.
      noteServerEcho(args[4] as string, 556)
      return Promise.resolve(undefined)
    })
    const detail = useTicketDetail(101)

    await detail.addComment({ ...reply(), attachments: [] })

    expect(isEchoSuppressed(556)).toBe(false)
    const tempId = (pool.upsert as ReturnType<typeof vi.fn>).mock.calls.find(
      ([kind, id]) => kind === 'comment' && (id as number) < 0,
    )![1]
    expect(pool.remove).toHaveBeenCalledWith('comment', tempId)
    expect(useTicketDraftsStore().getDraft(101).content).toBe('')
    expect(toast.error).not.toHaveBeenCalled()
  })
})
