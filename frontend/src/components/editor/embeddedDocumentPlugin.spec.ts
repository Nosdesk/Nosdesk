import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EditorState } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import * as Y from 'yjs'

// An embed previews a page's saved version, always revalidated: a snapshot
// fetched before the page's first save must not stick for the session, a
// failed refresh must not replace content on screen, and content that can't
// be rendered is not reported as an empty page.

const responses: Array<() => unknown> = []
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    get: vi.fn(async () => {
      const next = responses.shift()
      if (!next) throw new Error('unexpected fetch')
      return { data: next() }
    }),
  },
}))

import { schema } from './schema'
import { createEmbeddedDocumentPlugin } from './embeddedDocumentPlugin'

const settle = () => new Promise((r) => setTimeout(r, 0))

function pageWith(text: string | null) {
  const ydoc = new Y.Doc()
  if (text) {
    const p = new Y.XmlElement('paragraph')
    p.insert(0, [new Y.XmlText(text)])
    ydoc.getXmlFragment('prosemirror').insert(0, [p])
  }
  const bytes = Y.encodeStateAsUpdate(ydoc)
  return () => ({
    title: 'Runbook',
    icon: '📘',
    slug: 'runbook',
    yjs_document: btoa(String.fromCharCode(...bytes)),
  })
}

let uuidCounter = 0
let views: EditorView[] = []

function mountEmbed(uuid: string) {
  const doc = schema.node('doc', null, [
    schema.node('embedded_document', { documentUuid: uuid, documentTitle: 'Runbook' }),
  ])
  const view = new EditorView(document.createElement('div'), {
    state: EditorState.create({ doc, plugins: [createEmbeddedDocumentPlugin()] }),
  })
  views.push(view)
  return view.dom.querySelector('.embedded-document-block') as HTMLElement
}

beforeEach(() => {
  responses.length = 0
  uuidCounter++
})
afterEach(() => {
  views.forEach((v) => v.destroy())
  views = []
})

describe('embedded document preview', () => {
  it('shows content saved after an empty first fetch', async () => {
    const uuid = `page-${uuidCounter}`
    responses.push(pageWith(null))
    const first = mountEmbed(uuid)
    await settle()
    expect(first.textContent).toContain('Empty document')

    responses.push(pageWith('Restart the VPN'))
    const second = mountEmbed(uuid)
    await settle()
    expect(second.textContent).toContain('Restart the VPN')
  })

  it('paints the cached copy at once, and keeps it when a refresh fails', async () => {
    const uuid = `page-${uuidCounter}`
    responses.push(pageWith('Restart the VPN'))
    mountEmbed(uuid)
    await settle()

    responses.push(() => {
      throw new Error('network down')
    })
    const again = mountEmbed(uuid)
    // Before the refresh settles: the cached copy, no loading skeleton.
    expect(again.textContent).toContain('Restart the VPN')
    await settle()
    expect(again.textContent).toContain('Restart the VPN')
    expect(again.textContent).not.toContain("Couldn't load")
  })

  it('reports content it cannot render as such, not as an empty page', async () => {
    const uuid = `page-${uuidCounter}`
    const ydoc = new Y.Doc()
    // A node type this editor's schema doesn't know (e.g. written by a newer
    // editor) can't be rendered.
    ydoc.getXmlFragment('prosemirror').insert(0, [new Y.XmlElement('future_widget')])
    const bytes = Y.encodeStateAsUpdate(ydoc)
    responses.push(() => ({
      title: 'Runbook',
      icon: '📘',
      slug: 'runbook',
      yjs_document: btoa(String.fromCharCode(...bytes)),
    }))
    const embed = mountEmbed(uuid)
    await settle()
    expect(embed.textContent).toContain("Couldn't display this page")
    expect(embed.textContent).not.toContain('Empty document')
  })
})
