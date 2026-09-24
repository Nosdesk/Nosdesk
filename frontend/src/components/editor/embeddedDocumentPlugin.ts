import { Plugin, PluginKey } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import type { NodeView } from 'prosemirror-view'
import { Node as ProseMirrorNode } from 'prosemirror-model'
import { base64ToBytes, savedDocToHtml } from './savedDocHtml'
import apiClient from '@nosdesk/core/apiClient'
import { translate } from '@/i18n'

export const embeddedDocumentPluginKey = new PluginKey('embeddedDocument')

/**
 * An embed previews the page's last saved version. Results are kept per page
 * and always revalidated: a view paints the cached copy at once, then asks
 * again when it is created and when it scrolls back into view, and re-renders
 * only if something changed. Nothing is trusted for the whole session, so a
 * snapshot fetched before the page's first save can't stick.
 */
type EmbedState = 'content' | 'empty' | 'render-error' | 'load-error'

interface EmbeddedDocContent {
  state: EmbedState
  html: string
  title: string
  icon: string
  // Canonical slug for navigation. Docs are routed by slug, not uuid,
  // so the open affordance needs this to reach the page.
  slug: string | null
}

/** Last good result per page uuid (never a load error). */
const contentCache = new Map<string, EmbeddedDocContent>()
/** One fetch per page at a time, shared by every embed of it. */
const inflight = new Map<string, Promise<EmbeddedDocContent>>()

// Navigation callback. Receives the doc's slug (the routable path), not
// the uuid — the documentation route resolves by slug.
let navigateToDocument: ((slug: string) => void) | null = null

export function setDocumentNavigationHandler(handler: (slug: string) => void) {
  navigateToDocument = handler
}

function notice(key: string, fallback: string): string {
  return `<p class="text-tertiary text-sm italic">${translate(key, undefined, fallback)}</p>`
}

/** Fetch and render a page's saved content. Never throws. */
function fetchDocumentContent(uuid: string): Promise<EmbeddedDocContent> {
  const running = inflight.get(uuid)
  if (running) return running
  const run = (async (): Promise<EmbeddedDocContent> => {
    try {
      const { data } = await apiClient.get(`/documentation/pages/uuid/${uuid}/content`)
      const meta = {
        title: data.title || translate('docs-untitled-page', undefined, 'Untitled'),
        icon: data.icon || '📄',
        slug: data.slug ?? null,
      }
      let result: EmbeddedDocContent
      try {
        const html = data.yjs_document ? savedDocToHtml(base64ToBytes(data.yjs_document)) : ''
        result = html
          ? { state: 'content', html, ...meta }
          : { state: 'empty', html: notice('editor-embed-empty-document', 'Empty document'), ...meta }
      } catch (err) {
        // Distinct from empty: the page has content this editor can't show.
        console.error(`Failed to render embedded document ${uuid}:`, err)
        result = {
          state: 'render-error',
          html: notice('editor-embed-render-failed', "Couldn't display this page"),
          ...meta,
        }
      }
      contentCache.set(uuid, result)
      return result
    } catch (err) {
      console.error(`Failed to fetch embedded document ${uuid}:`, err)
      const loadFailed = translate('editor-embed-load-failed', undefined, "Couldn't load document")
      return {
        state: 'load-error',
        html: notice('editor-embed-load-failed', "Couldn't load document"),
        title: loadFailed,
        icon: '⚠️',
        slug: null,
      }
    } finally {
      inflight.delete(uuid)
    }
  })()
  inflight.set(uuid, run)
  return run
}

/** Revalidate at most this often per embed when it scrolls back into view. */
const REVALIDATE_ON_VIEW_MS = 10_000

// Custom NodeView for embedded_document nodes
class EmbeddedDocumentView implements NodeView {
  dom: HTMLElement
  private uuid: string
  private title: string
  private slug: string | null = null
  /** What is on screen, so a revalidation re-renders only on change. */
  private shown: EmbeddedDocContent | null = null
  private lastFetchedAt = 0
  private observer: IntersectionObserver | null = null

  constructor(node: ProseMirrorNode, _view: EditorView, _getPos: () => number | undefined) {
    this.uuid = node.attrs.documentUuid
    this.title = node.attrs.documentTitle || translate('docs-untitled-page', undefined, 'Untitled')

    this.dom = document.createElement('div')
    this.dom.className = 'embedded-document-block'
    this.dom.contentEditable = 'false'
    this.dom.setAttribute('data-embedded-document', 'true')
    this.dom.setAttribute('data-document-uuid', this.uuid)

    this.showCachedOrLoading()
    void this.revalidate()

    // Scrolling back to an embed asks again, so a long-open note catches up.
    if (typeof IntersectionObserver !== 'undefined') {
      this.observer = new IntersectionObserver((entries) => {
        if (!entries.some((e) => e.isIntersecting)) return
        if (Date.now() - this.lastFetchedAt < REVALIDATE_ON_VIEW_MS) return
        void this.revalidate()
      })
      this.observer.observe(this.dom)
    }
  }

  /** Paint the last good result at once, or the loading state if none. */
  private showCachedOrLoading() {
    const cached = contentCache.get(this.uuid)
    if (cached) {
      this.slug = cached.slug
      this.render(cached)
    } else {
      this.shown = null
      this.dom.replaceChildren(this.buildHeader('📄', this.title), this.buildSkeleton())
    }
  }

  private async revalidate() {
    const uuid = this.uuid
    this.lastFetchedAt = Date.now()
    const data = await fetchDocumentContent(uuid)
    // The node was pointed at another page while this ran.
    if (uuid !== this.uuid) return
    // A failed refresh never replaces content already on screen.
    if (data.state === 'load-error' && this.shown && this.shown.state !== 'load-error') return
    this.title = data.title
    this.slug = data.slug
    if (!sameContent(this.shown, data)) this.render(data)
  }

  // Navigate to the embedded doc. Docs route by slug, so resolve it
  // (known after the first fetch) before navigating; without a slug
  // there's nothing to open.
  private async openDocument() {
    if (!this.slug) {
      this.slug = (await fetchDocumentContent(this.uuid)).slug
    }
    if (this.slug && navigateToDocument) navigateToDocument(this.slug)
  }

  private render(data: EmbeddedDocContent) {
    this.shown = data
    const content = document.createElement('div')
    content.className = 'embedded-doc-content'
    content.innerHTML = data.html

    this.dom.replaceChildren(
      this.buildHeader(data.icon, data.title, data.state !== 'load-error'),
      content
    )
  }

  private buildHeader(icon: string, title: string, showOpen = false): HTMLElement {
    const header = document.createElement('div')
    header.className = 'embedded-doc-header'

    const iconEl = document.createElement('span')
    iconEl.className = 'embedded-doc-icon'
    iconEl.textContent = icon

    const titleEl = document.createElement('span')
    titleEl.className = 'embedded-doc-title'
    titleEl.textContent = title

    header.append(iconEl, titleEl)

    if (showOpen) {
      const openEl = document.createElement('a')
      openEl.className = 'embedded-doc-open'
      openEl.title = translate('editor-embed-open-document', undefined, 'Open document')
      openEl.innerHTML =
        '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">' +
          '<path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6" />' +
          '<polyline points="15,3 21,3 21,9" />' +
          '<line x1="10" y1="14" x2="21" y2="3" />' +
        '</svg>'
      // Real link so cmd/ctrl/middle-click opens the doc in a new tab;
      // a plain click is intercepted for SPA navigation.
      if (this.slug) openEl.setAttribute('href', `/documentation/${this.slug}`)
      openEl.addEventListener('click', (e) => {
        e.stopPropagation()
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return
        e.preventDefault()
        this.openDocument()
      })
      header.appendChild(openEl)
    }

    header.addEventListener('click', (e) => {
      if ((e.target as HTMLElement).closest('.embedded-doc-open')) return
      this.openDocument()
    })

    return header
  }

  private buildSkeleton(): HTMLElement {
    const content = document.createElement('div')
    content.className = 'embedded-doc-content'
    const skeleton = document.createElement('div')
    skeleton.className = 'embedded-doc-skeleton'
    for (const width of ['80%', '60%', '70%']) {
      const line = document.createElement('div')
      line.className = 'skeleton-line'
      line.style.width = width
      skeleton.appendChild(line)
    }
    content.appendChild(skeleton)
    return content
  }

  update(node: ProseMirrorNode): boolean {
    if (node.type.name !== 'embedded_document') return false
    const newUuid = node.attrs.documentUuid
    if (newUuid !== this.uuid) {
      this.uuid = newUuid
      this.title = node.attrs.documentTitle || translate('docs-untitled-page', undefined, 'Untitled')
      this.slug = null
      this.dom.setAttribute('data-document-uuid', newUuid)
      this.showCachedOrLoading()
      void this.revalidate()
    }
    return true
  }

  destroy() {
    this.observer?.disconnect()
    this.observer = null
  }

  stopEvent() {
    return true
  }

  ignoreMutation() {
    return true
  }
}

function sameContent(a: EmbeddedDocContent | null, b: EmbeddedDocContent): boolean {
  return (
    !!a &&
    a.state === b.state &&
    a.html === b.html &&
    a.title === b.title &&
    a.icon === b.icon &&
    a.slug === b.slug
  )
}

// Create the embedded document plugin
export function createEmbeddedDocumentPlugin(): Plugin {
  return new Plugin({
    key: embeddedDocumentPluginKey,
    props: {
      nodeViews: {
        embedded_document: (node, view, getPos) => new EmbeddedDocumentView(node, view, getPos)
      },
    }
  })
}
