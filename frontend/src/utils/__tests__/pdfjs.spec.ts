import { afterEach, describe, expect, it, vi } from 'vitest'

const workerOptions = vi.hoisted(() => ({ workerSrc: '' }))
vi.mock('pdfjs-dist', () => ({ GlobalWorkerOptions: workerOptions }))

import {
  PdfFetchError,
  PdfPasswordCancelled,
  awaitPdfDocument,
  classifyPdfError,
  fetchPdfBytes,
  loadPdfjs,
  openPdfDocument,
} from '@/utils/pdfjs'

type OnPassword = (update: (password: string) => void, reason: number) => void

// A stand-in for pdf.js: getDocument records its options and hands back a
// task whose promise the test settles.
function fakePdfjs() {
  let reject: (error: unknown) => void = () => {}
  const task = {
    promise: new Promise((_, r) => {
      reject = r
    }),
    onPassword: null as OnPassword | null,
    destroy: vi.fn(async () => {
      reject(Object.assign(new Error('Loading aborted'), { name: 'AbortException' }))
    }),
  }
  task.promise.catch(() => {})
  const getDocument = vi.fn((_options: Record<string, unknown>) => task)
  const pdfjs = { version: '6.4.299', getDocument, PasswordResponses: { NEED_PASSWORD: 1, INCORRECT_PASSWORD: 2 } }
  return { pdfjs: pdfjs as unknown as Parameters<typeof openPdfDocument>[0], getDocument, task }
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe('loadPdfjs', () => {
  it('points pdf.js at the minified worker, once', async () => {
    const first = await loadPdfjs()
    expect(workerOptions.workerSrc).toMatch(/pdf\.worker\.min\.mjs$/)
    expect(await loadPdfjs()).toBe(first)
  })
})

describe('openPdfDocument', () => {
  it("gives pdf.js this version's decoders, CMaps, fonts and colour profiles", () => {
    const { pdfjs, getDocument } = fakePdfjs()
    const data = new Uint8Array([1, 2, 3])
    openPdfDocument(pdfjs, data)

    const options = getDocument.mock.calls[0]![0]
    const base = `${window.location.origin}/pdfjs/6.4.299/`
    expect(options).toMatchObject({
      data,
      cMapPacked: true,
      wasmUrl: `${base}wasm/`,
      cMapUrl: `${base}cmaps/`,
      standardFontDataUrl: `${base}standard_fonts/`,
      iccUrl: `${base}iccs/`,
    })
  })

  it('asks for a password, and says when the last one was wrong', async () => {
    const { pdfjs, task } = fakePdfjs()
    const onPassword = vi.fn(async () => 'hunter2')
    openPdfDocument(pdfjs, new Uint8Array(), { onPassword })

    const update = vi.fn()
    task.onPassword!(update, 1)
    await vi.waitFor(() => expect(update).toHaveBeenCalledWith('hunter2'))
    expect(onPassword).toHaveBeenLastCalledWith('needed')

    task.onPassword!(update, 2)
    await vi.waitFor(() => expect(onPassword).toHaveBeenLastCalledWith('incorrect'))
  })

  it('ends the load as cancelled when the prompt is closed', async () => {
    const { pdfjs, task } = fakePdfjs()
    const loadingTask = openPdfDocument(pdfjs, new Uint8Array(), { onPassword: async () => null })

    task.onPassword!(vi.fn(), 1)
    await expect(awaitPdfDocument(loadingTask)).rejects.toBeInstanceOf(PdfPasswordCancelled)
    expect(task.destroy).toHaveBeenCalled()
  })

  it('leaves a protected PDF to fail on its own without a prompt', () => {
    const { pdfjs, task } = fakePdfjs()
    openPdfDocument(pdfjs, new Uint8Array())
    expect(task.onPassword).toBeNull()
  })
})

describe('fetchPdfBytes', () => {
  it("fetches without credentials, which the mobile app's file proxy refuses", async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(new Uint8Array([37, 80])))
    const bytes = await fetchPdfBytes('/api/files/tickets/7/a.pdf')
    expect(Array.from(bytes)).toEqual([37, 80])
    expect(fetchMock).toHaveBeenCalledWith('/api/files/tickets/7/a.pdf')
  })

  it('carries the HTTP status of a failed download', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response('', { status: 404 }))
    await expect(fetchPdfBytes('/x.pdf')).rejects.toMatchObject({ name: 'PdfFetchError', status: 404 })
  })

  it('reports a network failure without a status', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('Load failed'))
    await expect(fetchPdfBytes('/x.pdf')).rejects.toMatchObject({ name: 'PdfFetchError', status: null })
  })
})

describe('classifyPdfError', () => {
  it.each([
    [new PdfFetchError(404, ''), 'not-found'],
    [new PdfFetchError(403, ''), 'not-found'],
    [new PdfFetchError(null, ''), 'network'],
    [new PdfFetchError(500, ''), 'unknown'],
    [new PdfPasswordCancelled(), 'password-cancelled'],
    [Object.assign(new Error('Invalid PDF structure.'), { name: 'InvalidPDFException' }), 'invalid'],
    [new Error('boom'), 'unknown'],
    [undefined, 'unknown'],
  ])('%s is %s', (error, failure) => {
    expect(classifyPdfError(error)).toBe(failure)
  })
})
