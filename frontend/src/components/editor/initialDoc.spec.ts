/**
 * A note's content reaches its editor after the editor mounts: from IndexedDB,
 * which is asynchronous, or from the server over the WebSocket. Neither may
 * become the selection. When it did, the whole note sat selected, unfocused,
 * and the first keystroke that landed before a click took effect replaced it.
 */
import { describe, expect, it } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import * as Y from 'yjs'
import { ySyncPlugin } from 'y-prosemirror'

import { schema } from './schema'
import { initialEditorDoc } from './initialDoc'

const NOTE = 'main 1 incog 2 main 3 incog 114 main 114'

/** Another client's copy of the note, holding `text`. */
function peerWith(text: string): Y.Doc {
  const doc = new Y.Doc({ gc: false })
  const paragraph = new Y.XmlElement('paragraph')
  paragraph.insert(0, [new Y.XmlText(text)])
  doc.getXmlFragment('prosemirror').insert(0, [paragraph])
  return doc
}

/** A live editor bound to `ydoc`, as CollaborativeEditor builds it. */
function openEditor(ydoc: Y.Doc): EditorView {
  const fragment = ydoc.getXmlFragment('prosemirror')
  const { doc, mapping } = initialEditorDoc(fragment, schema)
  const mount = document.createElement('div')
  document.body.appendChild(mount)
  return new EditorView(mount, {
    state: EditorState.create({ doc, schema, plugins: [ySyncPlugin(fragment, { mapping })] }),
  })
}

/** What a keypress does: replace the current selection with the typed text. */
const type = (view: EditorView, text: string) => view.dispatch(view.state.tr.insertText(text))
const textOf = (ydoc: Y.Doc) => ydoc.getXmlFragment('prosemirror').toString()

describe('a note whose content arrives after its editor opens', () => {
  it('leaves a caret, not the whole note selected', () => {
    const local = new Y.Doc({ gc: false })
    const view = openEditor(local)

    Y.applyUpdate(local, Y.encodeStateAsUpdate(peerWith(NOTE)), 'remote')

    expect(view.state.doc.textContent).toBe(NOTE)
    expect(view.state.selection).toBeInstanceOf(TextSelection)
    expect(view.state.selection.empty).toBe(true)
  })

  it('keeps its text when the first keystroke lands', () => {
    const peer = peerWith(NOTE)
    const local = new Y.Doc({ gc: false })
    const view = openEditor(local)
    Y.applyUpdate(local, Y.encodeStateAsUpdate(peer), 'remote')

    type(view, ' after 117')
    // What reaches the other clients and the server.
    Y.applyUpdate(peer, Y.encodeStateAsUpdate(local, Y.encodeStateVector(peer)))

    expect(textOf(local)).toContain(NOTE)
    expect(textOf(peer)).toContain(NOTE)
    expect(textOf(peer)).toContain('after 117')
  })

  it('adds nothing to the shared note until someone types', () => {
    const local = new Y.Doc({ gc: false })
    const view = openEditor(local)
    // Transactions that change no content still run y-prosemirror's sync of
    // the editor into Yjs; the starting paragraph must not be written by it.
    view.dispatch(view.state.tr.setSelection(TextSelection.atEnd(view.state.doc)))
    view.dispatch(view.state.tr.setMeta('focus', true))
    expect(local.getXmlFragment('prosemirror').length).toBe(0)

    Y.applyUpdate(local, Y.encodeStateAsUpdate(peerWith(NOTE)), 'remote')
    expect(textOf(local)).toBe(`<paragraph>${NOTE}</paragraph>`)
  })
})

describe('a note this editor cannot show any of', () => {
  // A newer client can write a block an older build has no node for. The
  // editor then builds no blocks from the fragment, which is the same empty
  // document, with the same selection, as an empty fragment.
  it('still starts with a caret, so content arriving later is not selected', () => {
    const local = new Y.Doc({ gc: false })
    const unknown = new Y.XmlElement('no_such_node')
    unknown.insert(0, [new Y.XmlText('from a newer client')])
    local.getXmlFragment('prosemirror').insert(0, [unknown])
    const view = openEditor(local)
    expect(view.state.selection).toBeInstanceOf(TextSelection)

    const peer = peerWith(NOTE)
    Y.applyUpdate(local, Y.encodeStateAsUpdate(peer), 'remote')
    expect(view.state.selection.empty).toBe(true)
    type(view, ' after 117')
    expect(view.state.doc.textContent).toContain(NOTE)
  })
})

describe('a note whose content is already loaded', () => {
  it('opens showing it', () => {
    const local = peerWith(NOTE)
    const view = openEditor(local)
    expect(view.state.doc.textContent).toBe(NOTE)
    expect(view.state.selection.empty).toBe(true)
  })
})
