/**
 * Loads a PDF for the viewer: downloads it, opens it with pdf.js (asking for
 * a password when it has one), and releases it when the viewer is done.
 *
 * A load that finishes after a newer one started, or after the component
 * unmounted, releases its document instead of showing it.
 */
import { onBeforeUnmount, ref, shallowRef } from 'vue'
import type { PDFDocumentProxy } from 'pdfjs-dist'
import {
  awaitPdfDocument,
  fetchPdfBytes,
  loadPdfjs,
  openPdfDocument,
  type PdfPasswordRequest,
} from '@/utils/pdfjs'

interface UsePdfDocumentOptions {
  /** Asks for the password of a protected document. */
  onPassword?: PdfPasswordRequest
}

// pdf.js 6 releases a document, and ends its worker, through the task that
// loaded it.
async function releaseDocument(doc: PDFDocumentProxy) {
  try {
    await doc.loadingTask.destroy()
  } catch (err) {
    console.debug('PDF cleanup error:', err)
  }
}

export function usePdfDocument(options: UsePdfDocumentOptions = {}) {
  const pdfDocument = shallowRef<PDFDocumentProxy | null>(null)
  const isLoading = ref(false)
  const error = shallowRef<unknown>(null)

  let generation = 0
  let disposed = false

  /** Load `src`, replacing the current document. */
  const load = async (src: string) => {
    const mine = ++generation
    const isCurrent = () => !disposed && mine === generation
    isLoading.value = true
    error.value = null
    await close()

    try {
      const pdfjs = await loadPdfjs()
      const data = await fetchPdfBytes(src)
      if (!isCurrent()) return

      const doc = await awaitPdfDocument(openPdfDocument(pdfjs, data, { onPassword: options.onPassword }))
      if (!isCurrent()) {
        await releaseDocument(doc)
        return
      }
      pdfDocument.value = doc
    } catch (err) {
      if (isCurrent()) error.value = err
    } finally {
      if (isCurrent()) isLoading.value = false
    }
  }

  /** Release the current document. */
  const close = async () => {
    const doc = pdfDocument.value
    pdfDocument.value = null
    if (doc) await releaseDocument(doc)
  }

  onBeforeUnmount(() => {
    disposed = true
    // Supersede a load still in flight, so its document is released when it
    // arrives.
    generation++
    void close()
  })

  return { pdfDocument, isLoading, error, load, close }
}
