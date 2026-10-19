/**
 * Review's read-only preview of one converted page: a throwaway EditorView
 * over the editor's real schema, never the live editor (the pattern of
 * `components/editor/revisionView.ts`).
 *
 * The server sends the page as ProseMirror JSON. It is built with
 * `Node.fromJSON(schema, json)` and checked with `check()`; a page that
 * fails is a converter bug, reported as one and never shown half built.
 *
 *  - Images aren't stored yet, so each one is a named placeholder (file,
 *    size, and why it won't come across when it won't).
 *  - Links to other pages in the import (`nosdesk-import:page/{i}`) select
 *    that page instead of navigating; other links don't navigate either.
 *  - Spots the conversion changed carry a small label, gated on the page's
 *    note codes: a callout that became a quote, a checklist that became a
 *    bullet list, a table moved out of a list.
 */
import { Node as ProseMirrorNode, type Schema } from 'prosemirror-model'
import { EditorState, Plugin } from 'prosemirror-state'
import { Decoration, DecorationSet, EditorView, type NodeView } from 'prosemirror-view'

import type {
  DocsNoteCode,
  DocsPreviewImage,
} from '@nosdesk/core/services/docsImportService'

export const PAGE_LINK_PREFIX = 'nosdesk-import:page/'

export interface PreviewLabels {
  callout: string
  checklist: string
  tableMoved: string
  /** An image's meta line: size and alt, or its problem. */
  image: (info: DocsPreviewImage | undefined, alt: string | null) => string
  imageAria: (info: DocsPreviewImage | undefined, name: string) => string
}

export interface PreviewHandle {
  readonly view: EditorView
  destroy(): void
}

/** The page's document, built and checked against the schema. Throws when
 *  the JSON isn't a valid document. */
export function buildDoc(schema: Schema, json: unknown): ProseMirrorNode {
  const doc = ProseMirrorNode.fromJSON(schema, json as Record<string, unknown>)
  doc.check()
  return doc
}

const IMAGE_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>'
const IMAGE_OFF_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M21 15l-5-5L5 21"/><path d="M3 3l18 18"/></svg>'

class ImagePlaceholder implements NodeView {
  dom: HTMLElement
  constructor(node: ProseMirrorNode, images: Record<string, DocsPreviewImage>, labels: PreviewLabels) {
    const src = String(node.attrs.src ?? '')
    const info = images[src]
    const name = info?.name ?? (src.split('/').pop() || src)
    const dom = document.createElement('span')
    dom.className = 'docs-preview-img'
    if (info?.problem) dom.classList.add('is-problem')
    dom.setAttribute('role', 'img')
    dom.setAttribute('aria-label', labels.imageAria(info, name))
    dom.setAttribute('data-image', src)
    dom.innerHTML = info?.problem ? IMAGE_OFF_ICON : IMAGE_ICON
    const text = document.createElement('span')
    text.className = 'docs-preview-img__text'
    const nameEl = document.createElement('span')
    nameEl.className = 'docs-preview-img__name'
    nameEl.textContent = name
    const meta = document.createElement('span')
    meta.className = 'docs-preview-img__meta'
    meta.textContent = labels.image(info, (node.attrs.alt as string | null) ?? null)
    text.append(nameEl, meta)
    dom.append(text)
    this.dom = dom
  }
  stopEvent() {
    return true
  }
  ignoreMutation() {
    return true
  }
}

function firstText(node: ProseMirrorNode | null | undefined): string {
  return node?.firstChild?.firstChild?.textContent ?? ''
}

/** Decorations marking where the conversion changed the page. */
export function noteDecorations(
  doc: ProseMirrorNode,
  notes: Partial<Record<DocsNoteCode, number>>,
  labels: PreviewLabels,
): DecorationSet {
  const decos: Decoration[] = []
  let tablesToMark = notes.tables_hoisted ?? 0
  doc.descendants((node, pos) => {
    const name = node.type.name
    if (name === 'blockquote' && notes.callouts_as_quotes) {
      const first = node.firstChild
      const lead = first?.firstChild
      if (first?.type.name === 'paragraph' && lead?.marks.some((m) => m.type.name === 'strong')) {
        decos.push(Decoration.node(pos, pos + node.nodeSize, { class: 'docs-preview-note', 'data-note': labels.callout }))
      }
    }
    if (name === 'bullet_list' && notes.checklists_as_bullets && /^[☐☑]\s/.test(firstText(node))) {
      decos.push(Decoration.node(pos, pos + node.nodeSize, { class: 'docs-preview-note', 'data-note': labels.checklist }))
    }
    if (name === 'table' && tablesToMark > 0) {
      tablesToMark--
      decos.push(
        Decoration.node(pos, pos + node.nodeSize, {
          class: 'docs-preview-note is-warn',
          'data-note': labels.tableMoved,
        }),
      )
    }
    return true
  })
  return DecorationSet.create(doc, decos)
}

/** Mount the preview into `mount`. Throws, having cleaned up, when the
 *  document fails the schema check. */
export function mountPreview(opts: {
  mount: HTMLElement
  schema: Schema
  json: unknown
  images: Record<string, DocsPreviewImage>
  notes: Partial<Record<DocsNoteCode, number>>
  labels: PreviewLabels
  ariaLabel: string
  /** A link to another page in the import was followed. */
  onPageLink?: (index: number) => void
}): PreviewHandle {
  const doc = buildDoc(opts.schema, opts.json)
  const decorations = new Plugin({
    props: { decorations: (state) => noteDecorations(state.doc, opts.notes, opts.labels) },
  })
  const view = new EditorView(opts.mount, {
    state: EditorState.create({ doc, plugins: [decorations] }),
    editable: () => false,
    nodeViews: {
      image: (node) => new ImagePlaceholder(node, opts.images, opts.labels),
    },
    attributes: {
      class: 'docs-preview-content',
      'aria-readonly': 'true',
      'aria-label': opts.ariaLabel,
    },
    handleDOMEvents: {
      click(_view, event) {
        const link = (event.target as HTMLElement | null)?.closest('a')
        if (!link) return false
        event.preventDefault()
        const href = link.getAttribute('href') ?? ''
        if (href.startsWith(PAGE_LINK_PREFIX)) {
          const index = Number(href.slice(PAGE_LINK_PREFIX.length))
          if (Number.isInteger(index)) opts.onPageLink?.(index)
        }
        return true
      },
    },
  })
  let destroyed = false
  return {
    view,
    destroy() {
      if (destroyed) return
      destroyed = true
      view.destroy()
    },
  }
}
