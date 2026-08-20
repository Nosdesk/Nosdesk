/**
 * The non-destructiveness invariant for revision viewing.
 *
 * These tests exist because the previous implementation lost data. It called
 * `editorView.updateState()` on the LIVE editor with a plugin list that omitted
 * `ySyncPlugin`, which made ProseMirror destroy the plugin view and detach the
 * Yjs binding. Remote edits arriving while a revision was open never reached the
 * document, and exiting restored a pre-edit state that then overwrote the
 * collaborators' work.
 *
 * The assertions below are written against that failure mode specifically: they
 * pass with the detached overlay and fail if anyone re-states the live view.
 */
import { beforeEach, describe, expect, it } from 'vitest'
import { EditorState } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import * as Y from 'yjs'
import { initProseMirrorDoc, ySyncPlugin } from 'y-prosemirror'

import { schema } from './schema'
import { decodeRevisionBytes, mountRevisionView } from './revisionView'

/** Base64-encode bytes the way the revision endpoints do. */
function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (const b of bytes) binary += String.fromCharCode(b)
  return btoa(binary)
}

/** A live editor: Y.Doc + bound EditorView, as the app builds it. */
function makeLiveEditor() {
  const ydoc = new Y.Doc({ gc: false })
  const fragment = ydoc.getXmlFragment('prosemirror')
  const mount = document.createElement('div')
  document.body.appendChild(mount)

  const { doc, mapping } = initProseMirrorDoc(fragment, schema)
  const view = new EditorView(mount, {
    state: EditorState.create({ doc, schema, plugins: [ySyncPlugin(fragment, { mapping })] }),
  })
  return { ydoc, fragment, view, mount }
}

/** Type `text` as a new paragraph, the way an edit would arrive locally. */
function appendParagraph(view: EditorView, text: string) {
  const { state } = view
  const node = state.schema.nodes.paragraph.create(null, state.schema.text(text))
  view.dispatch(state.tr.insert(state.doc.content.size, node))
}

/**
 * A second peer editing the same logical document. Updates are exchanged by
 * hand so a test can control exactly when a "remote" edit lands.
 */
function makePeer(from: Y.Doc) {
  const peer = new Y.Doc({ gc: false })
  Y.applyUpdate(peer, Y.encodeStateAsUpdate(from))
  return {
    peer,
    editAndSyncTo(target: Y.Doc, text: string) {
      const frag = peer.getXmlFragment('prosemirror')
      const p = new Y.XmlElement('paragraph')
      p.insert(0, [new Y.XmlText(text)])
      frag.insert(frag.length, [p])
      Y.applyUpdate(target, Y.encodeStateAsUpdate(peer))
    },
  }
}

describe('mountRevisionView', () => {
  let host: HTMLElement

  beforeEach(() => {
    document.body.innerHTML = ''
    host = document.createElement('div')
    document.body.appendChild(host)
  })

  it('renders the revision content', () => {
    const source = new Y.Doc({ gc: false })
    const frag = source.getXmlFragment('prosemirror')
    const p = new Y.XmlElement('paragraph')
    p.insert(0, [new Y.XmlText('content as it was')])
    frag.insert(0, [p])

    const handle = mountRevisionView({
      mount: host,
      schema,
      updateBytes: Y.encodeStateAsUpdate(source),
    })

    expect(handle.text()).toContain('content as it was')
    handle.destroy()
  })

  it('is non-editable', () => {
    const source = new Y.Doc({ gc: false })
    const frag = source.getXmlFragment('prosemirror')
    const p = new Y.XmlElement('paragraph')
    p.insert(0, [new Y.XmlText('historical')])
    frag.insert(0, [p])

    const handle = mountRevisionView({ mount: host, schema, updateBytes: Y.encodeStateAsUpdate(source) })
    expect(handle.view.editable).toBe(false)
    handle.destroy()
  })

  // The core invariant. Under the old implementation this failed: the binding
  // was detached, so the remote paragraph never reached the ProseMirror doc, and
  // exiting reverted it in Yjs.
  it('lets remote edits reach the live document while a revision is open', () => {
    const live = makeLiveEditor()
    appendParagraph(live.view, 'original paragraph')
    const revisionBytes = Y.encodeStateAsUpdate(live.ydoc)

    const remote = makePeer(live.ydoc)

    const handle = mountRevisionView({ mount: host, schema, updateBytes: revisionBytes })

    // A collaborator edits while the revision is on screen.
    remote.editAndSyncTo(live.ydoc, 'arrived during revision view')

    expect(live.view.state.doc.textContent).toContain('arrived during revision view')

    handle.destroy()

    // ...and it is still there after exiting.
    expect(live.view.state.doc.textContent).toContain('arrived during revision view')
    expect(live.fragment.toString()).toContain('arrived during revision view')

    live.view.destroy()
  })

  it('leaves the live document byte-identical across mount and destroy', () => {
    const live = makeLiveEditor()
    appendParagraph(live.view, 'untouched')
    const before = Y.encodeStateAsUpdate(live.ydoc)

    const handle = mountRevisionView({ mount: host, schema, updateBytes: before })
    handle.destroy()

    const after = Y.encodeStateAsUpdate(live.ydoc)
    expect(Array.from(after)).toEqual(Array.from(before))

    live.view.destroy()
  })

  it('leaves the live editor state and selection untouched', () => {
    const live = makeLiveEditor()
    appendParagraph(live.view, 'keep my place')
    const stateBefore = live.view.state
    const selectionBefore = live.view.state.selection.from

    const handle = mountRevisionView({
      mount: host,
      schema,
      updateBytes: Y.encodeStateAsUpdate(live.ydoc),
    })
    handle.destroy()

    // Identity, not equality: the live view must not have been re-stated at all.
    expect(live.view.state).toBe(stateBefore)
    expect(live.view.state.selection.from).toBe(selectionBefore)
    expect(live.view.editable).toBe(true)

    live.view.destroy()
  })

  it('destroy is idempotent', () => {
    const source = new Y.Doc({ gc: false })
    source.getXmlFragment('prosemirror')
    const handle = mountRevisionView({ mount: host, schema, updateBytes: Y.encodeStateAsUpdate(source) })
    handle.destroy()
    expect(() => handle.destroy()).not.toThrow()
  })

  it('cleans up and rethrows when the update cannot be decoded', () => {
    const before = document.body.innerHTML
    expect(() =>
      mountRevisionView({ mount: host, schema, updateBytes: new Uint8Array([9, 9, 9, 9, 9]) }),
    ).toThrow()
    expect(document.body.innerHTML).toBe(before)
  })
})

describe('decodeRevisionBytes', () => {
  it('round-trips the wire encoding the revision endpoints use', () => {
    const source = new Y.Doc({ gc: false })
    const frag = source.getXmlFragment('prosemirror')
    const p = new Y.XmlElement('paragraph')
    p.insert(0, [new Y.XmlText('wire format')])
    frag.insert(0, [p])

    const bytes = Y.encodeStateAsUpdate(source)
    const decoded = decodeRevisionBytes(toBase64(bytes))

    expect(Array.from(decoded)).toEqual(Array.from(bytes))
  })
})
