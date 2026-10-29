/**
 * Print a PDF without leaving the app: draw each page to an image at print
 * resolution, lay the images out one per sheet in a hidden frame, and print
 * the frame. Printing the app's own page would print the viewer's chrome
 * and only the pages pdf.js happens to have drawn.
 *
 * Not for the mobile app: its webview has no print dialog. There, print
 * comes from the share sheet.
 */
import type { PDFDocumentProxy } from 'pdfjs-dist'

/** Dots per inch of the printed images; pdf.js's own print uses 150. */
const PRINT_DPI = 150

interface PrintOptions {
  /** Called after each page is drawn. */
  onProgress?: (done: number, total: number) => void
  /** Abort to stop preparing; nothing prints. */
  signal?: AbortSignal
}

// The frame of the last print, kept until its dialog closes. Safari returns
// from print() while the dialog is still open, so the frame can only go on
// afterprint (or when the next print starts, if that never fires).
let releasePrevious: (() => void) | null = null

/** Resolves once the print dialog has been shown (or the print was cancelled). */
export async function printPdfDocument(doc: PDFDocumentProxy, options: PrintOptions = {}): Promise<void> {
  const { onProgress, signal } = options
  releasePrevious?.()
  releasePrevious = null

  const frame = document.createElement('iframe')
  frame.setAttribute('aria-hidden', 'true')
  frame.tabIndex = -1
  frame.style.cssText = 'position:fixed;right:0;bottom:0;width:0;height:0;border:0;visibility:hidden'
  document.body.appendChild(frame)

  const urls: string[] = []
  const cleanUp = () => {
    urls.forEach((url) => URL.revokeObjectURL(url))
    frame.remove()
  }
  let printed = false

  try {
    const sheet = frame.contentDocument
    const win = frame.contentWindow
    if (!sheet || !win) throw new Error('Print frame unavailable')

    const first = (await doc.getPage(1)).getViewport({ scale: 1 })
    const style = sheet.createElement('style')
    // One image per sheet, the sheet sized to the first page.
    style.textContent =
      `@page { size: ${first.width}pt ${first.height}pt; margin: 0 }` +
      ' html, body { margin: 0; padding: 0 }' +
      ' img { display: block; width: 100%; height: auto; break-after: page }' +
      ' img:last-child { break-after: auto }'
    sheet.head.appendChild(style)

    const images: HTMLImageElement[] = []
    for (let number = 1; number <= doc.numPages; number++) {
      if (signal?.aborted) return
      const page = await doc.getPage(number)
      const viewport = page.getViewport({ scale: PRINT_DPI / 72 })
      const canvas = document.createElement('canvas')
      canvas.width = Math.floor(viewport.width)
      canvas.height = Math.floor(viewport.height)
      await page.render({ canvas, viewport, intent: 'print' }).promise
      const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
      canvas.width = 0
      canvas.height = 0
      page.cleanup()
      if (!blob) throw new Error(`Page ${number} could not be drawn`)

      const url = URL.createObjectURL(blob)
      urls.push(url)
      const image = sheet.createElement('img')
      // A blob: URL made here, nothing for assetUrl to resolve (and print
      // never runs in the mobile app).
      // eslint-disable-next-line no-restricted-syntax
      image.src = url
      image.alt = ''
      sheet.body.appendChild(image)
      images.push(image)
      onProgress?.(number, doc.numPages)
    }
    if (signal?.aborted) return

    await Promise.all(images.map((image) => image.decode().catch(() => {})))
    releasePrevious = cleanUp
    win.addEventListener(
      'afterprint',
      () => {
        if (releasePrevious === cleanUp) releasePrevious = null
        cleanUp()
      },
      { once: true },
    )
    printed = true
    win.focus()
    win.print()
  } finally {
    if (!printed) cleanUp()
  }
}
