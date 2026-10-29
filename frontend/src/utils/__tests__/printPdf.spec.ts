import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { printPdfDocument } from '@/utils/printPdf'

// A three-page document whose pages draw instantly.
function fakeDocument(pages = 3) {
  const page = {
    getViewport: ({ scale }: { scale: number }) => ({ width: 612 * scale, height: 792 * scale }),
    render: vi.fn((_params: unknown) => ({ promise: Promise.resolve() })),
    cleanup: vi.fn(),
  }
  return { doc: { numPages: pages, getPage: vi.fn(async () => page) }, page }
}

// The print frame is added synchronously; give its window a print() that
// records instead of opening jsdom's unimplemented dialog.
function catchFrame() {
  const print = vi.fn()
  const observer = new MutationObserver(() => {
    const frame = document.querySelector('iframe')
    const win = frame?.contentWindow as (Window & typeof globalThis) | null | undefined
    if (win && !('__patched' in win)) {
      Object.assign(win, { print, __patched: true })
      // The frame's images belong to its own window, which jsdom gives no decode().
      Object.defineProperty(win.HTMLImageElement.prototype, 'decode', { value: () => Promise.resolve(), configurable: true })
    }
  })
  observer.observe(document.body, { childList: true })
  return { print, stop: () => observer.disconnect() }
}

let urls = 0
beforeEach(() => {
  urls = 0
  vi.spyOn(HTMLCanvasElement.prototype, 'toBlob').mockImplementation((callback) => callback(new Blob(['png'])))
  vi.spyOn(URL, 'createObjectURL').mockImplementation(() => `blob:print/${++urls}`)
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
})
afterEach(() => {
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

describe('printPdfDocument', () => {
  it('lays every page out as an image, one per sheet, and prints the frame', async () => {
    const { doc, page } = fakeDocument()
    const frame = catchFrame()
    const progress: number[] = []
    await printPdfDocument(doc as never, { onProgress: (done) => progress.push(done) })
    frame.stop()

    const sheet = document.querySelector('iframe')!.contentDocument!
    expect([...sheet.querySelectorAll('img')].map((img) => img.getAttribute('src'))).toEqual([
      'blob:print/1',
      'blob:print/2',
      'blob:print/3',
    ])
    expect(sheet.querySelector('style')!.textContent).toContain('@page { size: 612pt 792pt; margin: 0 }')
    // 150 dpi: a Letter page is 1275 x 1650 pixels.
    const { intent, viewport } = page.render.mock.calls[0]![0] as { intent: string; viewport: { width: number; height: number } }
    expect(intent).toBe('print')
    expect(viewport.width).toBeCloseTo(1275)
    expect(viewport.height).toBeCloseTo(1650)
    expect(progress).toEqual([1, 2, 3])
    expect(frame.print).toHaveBeenCalledTimes(1)
  })

  it('removes the frame and its images once the dialog closes', async () => {
    const { doc } = fakeDocument()
    const frame = catchFrame()
    await printPdfDocument(doc as never)
    frame.stop()
    const iframe = document.querySelector('iframe')!
    const revoked = vi.mocked(URL.revokeObjectURL).mock.calls.length
    iframe.contentWindow!.dispatchEvent(new Event('afterprint'))
    expect(document.querySelector('iframe')).toBeNull()
    expect(vi.mocked(URL.revokeObjectURL).mock.calls.length - revoked).toBe(3)
  })

  it('prints nothing when cancelled while preparing', async () => {
    const { doc } = fakeDocument()
    const frame = catchFrame()
    const abort = new AbortController()
    await printPdfDocument(doc as never, { signal: abort.signal, onProgress: () => abort.abort() })
    frame.stop()
    expect(frame.print).not.toHaveBeenCalled()
    expect(document.querySelector('iframe')).toBeNull()
  })
})
