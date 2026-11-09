import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { AxiosRequestConfig } from 'axios'

interface Call {
  url: string
  form: FormData
  config: AxiosRequestConfig
  resolve: (v: unknown) => void
  reject: (e: unknown) => void
}
const calls: Call[] = []
const post = vi.fn((url: string, form: FormData, config: AxiosRequestConfig) => {
  return new Promise((resolve, reject) => {
    calls.push({ url, form, config, resolve, reject })
    config.signal?.addEventListener('abort', () =>
      reject(Object.assign(new Error('canceled'), { code: 'ERR_CANCELED' })),
    )
  })
})
vi.mock('@nosdesk/core/apiClient', () => ({
  default: { post: (...args: unknown[]) => post(...(args as Parameters<typeof post>)) },
}))

import { useTicketUiStore } from '@nosdesk/core/stores/ticketUi'
import { setTransportUploadCap } from '@nosdesk/core/services/uploadLimits'

beforeAll(() => {
  // jsdom has no object URLs; each staged file asks for one.
  let n = 0
  URL.createObjectURL = vi.fn(() => `blob:preview-${++n}`)
  URL.revokeObjectURL = vi.fn()
})
beforeEach(() => setActivePinia(createPinia()))
afterEach(() => {
  calls.length = 0
  setTransportUploadCap(null)
  vi.clearAllMocks()
})

const file = (name: string) => new File(['hello'], name, { type: 'text/plain' })
const stored = (id: number, name: string) => ({
  data: [{ id, url: `/uploads/temp/${name}`, name }],
})
/** Let the store's awaits run. */
const settle = () => new Promise((r) => setTimeout(r, 0))

describe('a staged file', () => {
  it('uploads at once, on its own request', async () => {
    const ui = useTicketUiStore()
    const [entry] = ui.stageFiles(101, [file('log.txt')])

    expect(entry.status).toBe('uploading')
    expect(calls).toHaveLength(1)
    expect(calls[0].url).toBe('/upload')
    expect((calls[0].form.get('files') as File).name).toBe('log.txt')
    expect(calls[0].config.timeout).toBe(0)

    calls[0].resolve(stored(7, 'log.txt'))
    await settle()
    expect(ui.getAttachments(101)[0]).toMatchObject({
      status: 'uploaded',
      uploaded: { id: 7, url: '/uploads/temp/log.txt' },
    })
  })

  it('sends a voice note with its transcription before the file', () => {
    const ui = useTicketUiStore()
    const note = Object.assign(new File(['a'], 'Voice Note.webm', { type: 'audio/webm' }), {
      _transcription: 'Printer is jammed',
    })
    ui.stageFiles(101, [note])

    expect([...calls[0].form.keys()]).toEqual(['transcription', 'files'])
    expect(calls[0].form.get('transcription')).toBe('Printer is jammed')
  })

  it('reports its progress', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('clip.mov')])
    const entry = ui.getAttachments(101)[0]

    calls[0].config.onUploadProgress?.({ loaded: 25, total: 100 } as never)
    expect(entry).toMatchObject({ loaded: 25, total: 100 })
    calls[0].config.onUploadProgress?.({ loaded: 60, total: 100 } as never)
    expect(entry.loaded).toBe(60)
    // A transport that can't tell the size leaves the total unknown.
    calls[0].config.onUploadProgress?.({ loaded: 70 } as never)
    expect(entry.total).toBeNull()
  })

  it('waits its turn: two upload at once', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt'), file('b.txt'), file('c.txt')])
    const [a, b, c] = ui.getAttachments(101)

    expect([a.status, b.status, c.status]).toEqual(['uploading', 'uploading', 'queued'])
    expect(calls).toHaveLength(2)

    calls[0].resolve(stored(1, 'a.txt'))
    await settle()
    expect(c.status).toBe('uploading')
    expect(calls).toHaveLength(3)
  })

  it('goes one at a time in the app, whose bridge copies each body', async () => {
    setTransportUploadCap(25 * 1024 * 1024)
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt'), file('b.txt')])

    expect(calls).toHaveLength(1)
    calls[0].resolve(stored(1, 'a.txt'))
    await settle()
    expect(calls).toHaveLength(2)
  })
})

describe('removing a staged file', () => {
  it('cancels its upload while it runs', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('big.mov')])
    const entry = ui.getAttachments(101)[0]

    ui.removeAttachment(101, entry.key)
    await settle()

    expect(calls[0].config.signal?.aborted).toBe(true)
    expect(ui.getAttachments(101)).toEqual([])
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(entry.previewUrl)
  })

  it('never starts one that was waiting', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt'), file('b.txt'), file('c.txt')])
    const waiting = ui.getAttachments(101)[2]

    ui.removeAttachment(101, waiting.key)
    calls[0].resolve(stored(1, 'a.txt'))
    calls[1].resolve(stored(2, 'b.txt'))
    await settle()

    expect(calls).toHaveLength(2)
    expect(ui.getAttachments(101).map((a) => a.file.name)).toEqual(['a.txt', 'b.txt'])
  })

  it('drops an uploaded one without asking the server', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('done.txt')])
    calls[0].resolve(stored(7, 'done.txt'))
    await settle()

    ui.removeAttachment(101, ui.getAttachments(101)[0].key)

    expect(post).toHaveBeenCalledTimes(1)
    expect(ui.getAttachments(101)).toEqual([])
  })

  it('frees its slot for the next file', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt'), file('b.txt'), file('c.txt')])
    ui.removeAttachment(101, ui.getAttachments(101)[0].key)
    await settle()

    expect(calls).toHaveLength(3)
    expect(ui.getAttachments(101).map((a) => a.status)).toEqual(['uploading', 'uploading'])
  })

  it('with the ticket stops every upload', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt'), file('b.txt'), file('c.txt')])
    ui.clearAttachments(101)
    await settle()

    expect(calls).toHaveLength(2)
    expect(calls.every((c) => c.config.signal?.aborted)).toBe(true)
    expect(ui.getAttachments(101)).toEqual([])
  })
})

describe('a failed upload', () => {
  it('says why, by the server code or status', async () => {
    const cases: [unknown, string][] = [
      [{ response: { status: 413, data: { code: 'FILE_TOO_LARGE', file: 'a' } } }, 'too-large'],
      [{ response: { status: 413, data: {} } }, 'too-large'],
      [{ code: 'ERR_UPLOAD_TOO_LARGE' }, 'too-large'],
      [{ response: { status: 400, data: { code: 'FILE_TYPE_NOT_ALLOWED', file: 'a' } } }, 'type-not-allowed'],
      [{ response: { status: 400, data: { code: 'INVALID_FILENAME', file: 'a' } } }, 'invalid-name'],
      [{ response: { status: 503, data: { code: 'SERVICE_UNAVAILABLE' } } }, 'busy'],
      [{ code: 'ERR_NETWORK', message: 'Network Error' }, 'offline'],
      [{ response: { status: 500, data: { code: 'INTERNAL_ERROR' } } }, 'failed'],
    ]
    const ui = useTicketUiStore()
    for (const [err, failure] of cases) {
      const [entry] = ui.stageFiles(101, [file('a.txt')])
      calls.at(-1)!.reject(err)
      await settle()
      expect(entry).toMatchObject({ status: 'failed', failure })
    }
  })

  it('uploads again on retry', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt')])
    const entry = ui.getAttachments(101)[0]
    calls[0].reject({ response: { status: 503, data: { code: 'SERVICE_UNAVAILABLE' } } })
    await settle()
    expect(entry.status).toBe('failed')

    ui.retryAttachment(101, entry.key)
    expect(entry).toMatchObject({ status: 'uploading', failure: null, loaded: 0 })
    expect(calls).toHaveLength(2)

    calls[1].resolve(stored(9, 'a.txt'))
    await settle()
    expect(entry).toMatchObject({ status: 'uploaded', uploaded: { id: 9 } })
  })

  it('counts an answer without the stored file as a failure', async () => {
    const ui = useTicketUiStore()
    const [entry] = ui.stageFiles(101, [file('a.txt')])
    calls[0].resolve({ data: [] })
    await settle()
    expect(entry).toMatchObject({ status: 'failed', failure: 'failed' })
  })
})

describe('sending', () => {
  it('takes the files off the composer with their previews intact', async () => {
    const ui = useTicketUiStore()
    ui.stageFiles(101, [file('a.txt')])
    calls[0].resolve(stored(7, 'a.txt'))
    await settle()

    const taken = ui.takeAttachments(101)

    expect(taken).toHaveLength(1)
    expect(ui.getAttachments(101)).toEqual([])
    expect(URL.revokeObjectURL).not.toHaveBeenCalled()

    // A reply that fails comes back before anything staged since.
    ui.stageFiles(101, [file('later.txt')])
    ui.restoreAttachments(101, taken)
    expect(ui.getAttachments(101).map((a) => a.file.name)).toEqual(['a.txt', 'later.txt'])
  })
})
