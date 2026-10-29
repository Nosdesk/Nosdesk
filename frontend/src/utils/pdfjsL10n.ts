/**
 * Translations for the text pdf.js puts in its own markup, from the app's
 * catalogue instead of pdf.js's built-in English.
 *
 * pdf.js marks the elements it labels with `data-l10n-id` (and arguments in
 * `data-l10n-args`) and asks its L10n object to fill them in. A read-only
 * viewer (no annotation editor) uses only the four labels below; this fills
 * those, and watches the viewer for new ones as pages render.
 */

type Translate = (key: string, args?: Record<string, string | number>) => string

interface Label {
  key: string
  /** The attribute the label goes in; the element's text when absent. */
  attr?: string
}

const LABELS: Record<string, Label> = {
  'pdfjs-page-landmark': { key: 'ticket-media-pdf-page-landmark', attr: 'aria-label' },
  'pdfjs-text-annotation-type': { key: 'ticket-media-pdf-annotation-type', attr: 'alt' },
  'pdfjs-show-comment-button': { key: 'ticket-media-pdf-show-comment', attr: 'title' },
  // Formatted here rather than through the catalogue: the date is the whole
  // message.
  'pdfjs-annotation-date-time-string': { key: '' },
}

const L10N_SELECTOR = '[data-l10n-id]'

/**
 * The object pdf.js's PDFViewer takes as `l10n`. `translate(element)` fills
 * the labels under `element` now and as they appear.
 */
export function createPdfjsL10n(translate: Translate, lang: string) {
  const observers = new Map<Element, MutationObserver>()
  const dateFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'medium' })

  const message = (id: string, args: Record<string, unknown> | null): string | null => {
    const label = LABELS[id]
    if (!label) return null
    if (id === 'pdfjs-annotation-date-time-string') {
      const time = Number(args?.dateObj)
      return Number.isFinite(time) ? dateFormat.format(new Date(time)) : ''
    }
    return translate(label.key, (args ?? undefined) as Record<string, string | number> | undefined)
  }

  const fill = (element: Element) => {
    const id = element.getAttribute('data-l10n-id')
    if (!id) return
    let args: Record<string, unknown> | null = null
    try {
      args = JSON.parse(element.getAttribute('data-l10n-args') ?? 'null')
    } catch {
      args = null
    }
    const text = message(id, args)
    if (text === null) return
    const attr = LABELS[id]?.attr
    if (attr) element.setAttribute(attr, text)
    else element.textContent = text
  }

  const fillAll = (root: Element) => {
    if (root.matches(L10N_SELECTOR)) fill(root)
    root.querySelectorAll(L10N_SELECTOR).forEach(fill)
  }

  return {
    getLanguage: () => lang,
    getDirection: () => (document.documentElement.dir === 'rtl' ? 'rtl' : 'ltr'),
    async get(ids: string | string[], args: Record<string, unknown> | null = null) {
      if (Array.isArray(ids)) return ids.map((id) => message(id, null) ?? '')
      return message(ids, args) ?? ''
    },
    async translate(element: Element) {
      fillAll(element)
      if (observers.has(element)) return
      const observer = new MutationObserver((records) => {
        for (const record of records) {
          if (record.type === 'attributes') fill(record.target as Element)
          else record.addedNodes.forEach((node) => node instanceof Element && fillAll(node))
        }
      })
      observer.observe(element, {
        subtree: true,
        childList: true,
        attributes: true,
        attributeFilter: ['data-l10n-id', 'data-l10n-args'],
      })
      observers.set(element, observer)
    },
    async translateOnce(element: Element) {
      fillAll(element)
    },
    async destroy() {
      observers.forEach((observer) => observer.disconnect())
      observers.clear()
    },
    pause() {},
    resume() {},
  }
}
