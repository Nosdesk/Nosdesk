/**
 * Opens and closes the PDF overlay.
 *
 * A saved file opens through the URL: `?pdf=<stored path>` (and `&page=N`)
 * on the current route. Opening pushes a history entry, so Back closes the
 * overlay, a reload keeps it open, and a copied link opens the same view
 * with the PDF on top. A file not saved yet (a composer preview, a `blob:`
 * URL) has no path to put in a link, so it opens from memory and closes on
 * reload.
 */
import { computed, shallowRef } from 'vue'
import type { LocationQuery, RouteLocationNormalizedLoaded, Router } from 'vue-router'
import { isAllowedPdfPath, pdfNameFromPath } from '@/utils/pdfSource'

export interface PdfOverlaySource {
  /** The stored path, or a `blob:` URL for an unsaved file. */
  src: string
  name: string | null
  page: number | null
  /** Opened from memory, not from the URL. */
  local: boolean
}

const localSource = shallowRef<PdfOverlaySource | null>(null)

const withoutPdf = (query: LocationQuery): LocationQuery => {
  const { pdf: _pdf, page: _page, ...rest } = query
  return rest
}

/** The PDF the URL asks the overlay to show, if it's one it may open. */
export function pdfFromQuery(query: LocationQuery): PdfOverlaySource | null {
  const path = query.pdf
  if (!isAllowedPdfPath(path)) return null
  const page = Number(query.page)
  return {
    src: path,
    name: pdfNameFromPath(path),
    page: Number.isInteger(page) && page > 0 ? page : null,
    local: false,
  }
}

export function usePdfOverlay(router: Router, route: RouteLocationNormalizedLoaded) {
  const source = computed(() => localSource.value ?? pdfFromQuery(route.query))

  /** Open a saved file by its stored path, or an unsaved one by its blob URL. */
  const open = (file: { src: string; name: string }) => {
    if (file.src.startsWith('blob:')) {
      localSource.value = { src: file.src, name: file.name, page: null, local: true }
      return
    }
    if (!isAllowedPdfPath(file.src)) return
    void router.push({ query: { ...withoutPdf(route.query), pdf: file.src } })
  }

  /**
   * Close the overlay. When opening it pushed the entry we're on, going back
   * returns to the page as it was; a link opened cold has nothing to go back
   * to, so the query is dropped in place.
   */
  const close = () => {
    if (localSource.value) {
      localSource.value = null
      return
    }
    if (!route.query.pdf) return
    const target = router.resolve({ query: withoutPdf(route.query) })
    const back = router.options.history.state.back
    if (typeof back === 'string' && back === target.fullPath) {
      router.back()
    } else {
      void router.replace(target)
    }
  }

  return { source, open, close }
}
