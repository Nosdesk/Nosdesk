/**
 * usePdfViewer - Vue 3 composable for scrollable PDF viewing with pdf.js
 *
 * Features:
 * - Scrollable multi-page view with lazy rendering
 * - IntersectionObserver-based page visibility detection
 * - DPI-aware canvas rendering
 * - Proper memory management
 */

import { ref, shallowRef, onBeforeUnmount, watch } from 'vue'
import type { PDFDocumentProxy } from 'pdfjs-dist'

const PDF_CONFIG = {
  MIN_SCALE: 0.5,
  MAX_SCALE: 3.0,
  SCALE_STEP: 0.1,
} as const

interface PageDimensions {
  width: number
  height: number
}

interface UsePdfViewerOptions {
  onError?: (error: Error) => void
  onReady?: () => void
}

export function usePdfViewer(options: UsePdfViewerOptions = {}) {
  const { onError, onReady } = options

  // Core state
  const pdfDocument = shallowRef<PDFDocumentProxy | null>(null)
  const isLoading = ref(false)
  const totalPages = ref(0)
  const scale = ref(1.0)
  const pageDimensions = ref<PageDimensions[]>([])
  const maxPageWidth = ref(0)
  const renderedPages = ref<Set<number>>(new Set())
  const renderingPages = ref<Set<number>>(new Set())

  // Initialize PDF.js library
  const initPdfJs = async () => {
    const pdfjsLib = await import('pdfjs-dist')
    pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(
      'pdfjs-dist/build/pdf.worker.mjs',
      import.meta.url
    ).href
    return pdfjsLib
  }

  // Load PDF document - only get first page dimensions initially (memory efficient)
  const loadDocument = async (src: string) => {
    isLoading.value = true
    renderedPages.value.clear()
    renderingPages.value.clear()

    try {
      // Cleanup previous document
      if (pdfDocument.value) {
        await pdfDocument.value.cleanup?.()
        await pdfDocument.value.destroy?.()
        pdfDocument.value = null
      }

      const pdfjsLib = await initPdfJs()

      // Fetch with authentication
      const response = await fetch(src, { credentials: 'include' })
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`)
      }

      const arrayBuffer = await response.arrayBuffer()
      const loadingTask = pdfjsLib.getDocument({ data: new Uint8Array(arrayBuffer) })
      pdfDocument.value = await loadingTask.promise
      totalPages.value = pdfDocument.value.numPages

      // Get first two page dimensions (cover pages often differ from content pages)
      // This gives a better estimate for layout without loading all pages
      const firstPage = await pdfDocument.value.getPage(1)
      const firstViewport = firstPage.getViewport({ scale: 1.0 })
      const firstDims = { width: firstViewport.width, height: firstViewport.height }

      let defaultDims = firstDims
      let maxWidth = firstViewport.width

      if (totalPages.value >= 2) {
        const secondPage = await pdfDocument.value.getPage(2)
        const secondViewport = secondPage.getViewport({ scale: 1.0 })
        // Use the larger width as the default estimate
        if (secondViewport.width > firstViewport.width) {
          defaultDims = { width: secondViewport.width, height: secondViewport.height }
          maxWidth = secondViewport.width
        }
      }

      // Initialize all pages with the larger dimension as estimate
      pageDimensions.value = Array(totalPages.value).fill(null).map((_, i) =>
        i === 0 ? firstDims : defaultDims
      )
      maxPageWidth.value = maxWidth

      onReady?.()
    } catch (error) {
      onError?.(error as Error)
      throw error
    } finally {
      isLoading.value = false
    }
  }

  // Render a single page to its canvas
  const renderPage = async (pageNum: number, canvas: HTMLCanvasElement): Promise<boolean> => {
    if (!pdfDocument.value) return false
    if (pageNum < 1 || pageNum > totalPages.value) return false
    if (renderedPages.value.has(pageNum)) return true
    if (renderingPages.value.has(pageNum)) return false

    // Limit concurrent renders to prevent memory issues
    if (renderingPages.value.size >= 3) return false

    renderingPages.value.add(pageNum)

    try {
      const page = await pdfDocument.value.getPage(pageNum)
      const baseViewport = page.getViewport({ scale: 1.0 })

      // Update actual page dimensions (may differ from initial estimate)
      if (pageDimensions.value[pageNum - 1]) {
        pageDimensions.value[pageNum - 1] = {
          width: baseViewport.width,
          height: baseViewport.height
        }
        // Update max width if this page is wider
        if (baseViewport.width > maxPageWidth.value) {
          maxPageWidth.value = baseViewport.width
        }
      }

      const viewport = page.getViewport({ scale: scale.value })
      const context = canvas.getContext('2d')
      if (!context) throw new Error('Could not get canvas context')

      // DPI-aware sizing (cap at 2x for memory efficiency)
      const pixelRatio = Math.min(window.devicePixelRatio || 1, 2)
      canvas.width = viewport.width * pixelRatio
      canvas.height = viewport.height * pixelRatio
      canvas.style.width = `${viewport.width}px`
      canvas.style.height = `${viewport.height}px`

      context.scale(pixelRatio, pixelRatio)

      await page.render({
        canvasContext: context,
        viewport,
      }).promise

      renderedPages.value.add(pageNum)
      return true
    } catch (error) {
      if ((error as any)?.name !== 'RenderingCancelledException') {
        console.error(`Error rendering page ${pageNum}:`, error)
      }
      return false
    } finally {
      renderingPages.value.delete(pageNum)
    }
  }

  // Get scaled dimensions for a page
  const getPageDimensions = (pageNum: number): PageDimensions | null => {
    const dims = pageDimensions.value[pageNum - 1]
    if (!dims) return null

    return {
      width: dims.width * scale.value,
      height: dims.height * scale.value,
    }
  }

  // Get the max width scaled (for consistent wrapper sizing)
  const getScaledMaxWidth = (): number => {
    return maxPageWidth.value * scale.value
  }

  // Clear rendered pages (needed when scale changes)
  const clearRenderedPages = () => {
    renderedPages.value.clear()
  }

  // Zoom controls
  const zoomIn = () => {
    scale.value = Math.min(PDF_CONFIG.MAX_SCALE, scale.value + PDF_CONFIG.SCALE_STEP)
  }

  const zoomOut = () => {
    scale.value = Math.max(PDF_CONFIG.MIN_SCALE, scale.value - PDF_CONFIG.SCALE_STEP)
  }

  const resetZoom = () => {
    scale.value = 1.0
  }

  // Fit to container width - returns the scale needed
  const fitToWidth = (containerWidth: number, padding: number = 32) => {
    if (maxPageWidth.value <= 0) return
    const availableWidth = containerWidth - padding
    const newScale = Math.min(PDF_CONFIG.MAX_SCALE, Math.max(PDF_CONFIG.MIN_SCALE, availableWidth / maxPageWidth.value))
    scale.value = Math.round(newScale * 100) / 100 // Round to 2 decimal places
  }

  // Clear rendered pages when scale changes
  watch(scale, () => {
    clearRenderedPages()
  }, { flush: 'sync' })

  // Cleanup on unmount
  const cleanup = async () => {
    if (pdfDocument.value) {
      try {
        await pdfDocument.value.cleanup?.()
        await pdfDocument.value.destroy?.()
      } catch (err) {
        console.debug('PDF cleanup error:', err)
      }
      pdfDocument.value = null
    }
    totalPages.value = 0
    pageDimensions.value = []
    maxPageWidth.value = 0
    renderedPages.value.clear()
    renderingPages.value.clear()
  }

  onBeforeUnmount(() => {
    cleanup()
  })

  return {
    // State
    isLoading,
    totalPages,
    scale,
    pageDimensions,
    maxPageWidth,
    renderedPages,
    // Actions
    loadDocument,
    renderPage,
    getPageDimensions,
    getScaledMaxWidth,
    clearRenderedPages,
    zoomIn,
    zoomOut,
    resetZoom,
    fitToWidth,
    cleanup,
  }
}
