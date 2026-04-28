import { Plugin, PluginKey } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import type { NodeView } from 'prosemirror-view'
import { Node as ProseMirrorNode, DOMSerializer } from 'prosemirror-model'
import { schema } from './schema'
import { sanitiseHtml } from '@/composables/useSanitise'
import apiClient from '@/services/apiConfig'
import * as Y from 'yjs'

export const embeddedDocumentPluginKey = new PluginKey('embeddedDocument')

// Cache for embedded document content
const contentCache = new Map<string, { html: string; title: string; icon: string; loading: boolean; error: boolean }>()

// Navigation callback
let navigateToDocument: ((uuid: string) => void) | null = null

export function setDocumentNavigationHandler(handler: (uuid: string) => void) {
  navigateToDocument = handler
}

// Fetch document content for embedding
async function fetchDocumentContent(uuid: string): Promise<{ html: string; title: string; icon: string }> {
  const cached = contentCache.get(uuid)
  if (cached && !cached.loading && !cached.error) {
    return cached
  }

  contentCache.set(uuid, { html: '', title: 'Loading...', icon: '📄', loading: true, error: false })

  try {
    const response = await apiClient.get(`/documentation/pages/uuid/${uuid}/content`)
    const data = response.data

    let html = ''
    if (data.yjs_document) {
      // Decode base64 Yjs document and render to HTML
      const binaryData = Uint8Array.from(atob(data.yjs_document), c => c.charCodeAt(0))
      const ydoc = new Y.Doc()
      Y.applyUpdate(ydoc, binaryData)

      // Get the prosemirror XML fragment and serialize to HTML
      const xmlFragment = ydoc.getXmlFragment('prosemirror')
      html = xmlFragmentToHtml(xmlFragment)
      ydoc.destroy()
    }

    if (!html) {
      html = '<p class="text-tertiary text-sm italic">Empty document</p>'
    }

    const result = {
      html,
      title: data.title || 'Untitled',
      icon: data.icon || '📄',
      loading: false,
      error: false,
    }
    contentCache.set(uuid, result)
    return result
  } catch (err) {
    console.error(`Failed to fetch embedded document ${uuid}:`, err)
    const errorResult = {
      html: '<p class="text-tertiary text-sm italic">Could not load document</p>',
      title: 'Error loading document',
      icon: '⚠️',
      loading: false,
      error: true,
    }
    contentCache.set(uuid, errorResult)
    return errorResult
  }
}

// Convert Yjs XmlFragment to HTML via ProseMirror's DOMSerializer.
// Converts XmlFragment → ProseMirror JSON → ProseMirror Node → DOM → HTML,
// so that schema toDOM methods handle all tag mapping and mark rendering.
function xmlFragmentToHtml(fragment: Y.XmlFragment): string {
  const content = fragment.toArray().flatMap(child => {
    if (child instanceof Y.XmlElement) return [xmlElementToJSON(child)]
    return []
  })
  if (content.length === 0) return ''

  try {
    const doc = ProseMirrorNode.fromJSON(schema, { type: 'doc', content })
    const serializer = DOMSerializer.fromSchema(schema)
    const dom = serializer.serializeFragment(doc.content)
    const wrapper = document.createElement('div')
    wrapper.appendChild(dom)
    return sanitiseHtml(wrapper.innerHTML)
  } catch (err) {
    console.error('Failed to serialize embedded document content:', err)
    return ''
  }
}

function xmlElementToJSON(element: Y.XmlElement): any {
  const type = element.nodeName
  if (!type || type === 'undefined') return null

  const attrs: Record<string, any> = {}
  for (const [key, value] of Object.entries(element.getAttributes())) {
    if (key !== 'ychange') attrs[key] = value
  }

  const content: any[] = []
  for (const child of element.toArray()) {
    if (child instanceof Y.XmlElement) {
      const json = xmlElementToJSON(child)
      if (json) content.push(json)
    } else if (child instanceof Y.XmlText) {
      for (const delta of child.toDelta()) {
        const textNode: any = { type: 'text', text: delta.insert }
        if (delta.attributes) {
          textNode.marks = Object.entries(delta.attributes)
            .filter(([k]) => k !== 'ychange')
            .map(([markType, value]) => {
              if (typeof value === 'object' && value !== null) {
                return { type: markType, attrs: value }
              }
              return { type: markType }
            })
        }
        if (textNode.text) content.push(textNode)
      }
    }
  }

  const node: any = { type }
  if (Object.keys(attrs).length > 0) node.attrs = attrs
  if (content.length > 0) node.content = content
  return node
}

// Custom NodeView for embedded_document nodes
class EmbeddedDocumentView implements NodeView {
  dom: HTMLElement
  private uuid: string
  private title: string

  constructor(node: ProseMirrorNode, _view: EditorView, _getPos: () => number | undefined) {
    this.uuid = node.attrs.documentUuid
    this.title = node.attrs.documentTitle || 'Untitled'

    this.dom = document.createElement('div')
    this.dom.className = 'embedded-document-block'
    this.dom.contentEditable = 'false'
    this.dom.setAttribute('data-embedded-document', 'true')
    this.dom.setAttribute('data-document-uuid', this.uuid)

    // Render loading state
    this.renderLoading()

    // Fetch and render content
    this.loadContent()
  }

  private renderLoading() {
    this.dom.replaceChildren(
      this.buildHeader('📄', this.title),
      this.buildSkeleton()
    )
  }

  private async loadContent() {
    const data = await fetchDocumentContent(this.uuid)
    this.title = data.title
    this.render(data)
  }

  private render(data: { html: string; title: string; icon: string }) {
    const content = document.createElement('div')
    content.className = 'embedded-doc-content'
    content.innerHTML = data.html

    this.dom.replaceChildren(
      this.buildHeader(data.icon, data.title, true),
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
      openEl.title = 'Open document'
      openEl.innerHTML =
        '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">' +
          '<path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6" />' +
          '<polyline points="15,3 21,3 21,9" />' +
          '<line x1="10" y1="14" x2="21" y2="3" />' +
        '</svg>'
      openEl.addEventListener('click', (e) => {
        e.preventDefault()
        e.stopPropagation()
        if (navigateToDocument) navigateToDocument(this.uuid)
      })
      header.appendChild(openEl)
    }

    header.addEventListener('click', (e) => {
      if ((e.target as HTMLElement).closest('.embedded-doc-open')) return
      if (navigateToDocument) navigateToDocument(this.uuid)
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
      this.title = node.attrs.documentTitle || 'Untitled'
      // Clear cache for re-fetch
      contentCache.delete(newUuid)
      this.renderLoading()
      this.loadContent()
    }
    return true
  }

  destroy() {
    // Cleanup
  }

  stopEvent() {
    return true
  }

  ignoreMutation() {
    return true
  }
}

// Invalidate cache for a specific document (called when SSE events fire)
export function invalidateEmbeddedDocCache(uuid: string) {
  contentCache.delete(uuid)
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
