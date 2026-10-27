/**
 * The server's Markdown converter (`backend/src/services/docs_markdown`) must
 * write documents the editor can build. y-prosemirror deletes any Yjs element
 * it cannot turn into a ProseMirror node, from the shared document, for
 * everyone, on the first open, so an invalid import destroys content rather
 * than failing loudly.
 *
 * Cargo writes the golden fixtures (`backend/tests/fixtures/docs_markdown`,
 * regenerated with `UPDATE_DOCS_MARKDOWN_FIXTURES=1 cargo test docs_markdown`).
 * Each is loaded here exactly as the editor loads a page, checked against the
 * real schema, and its ProseMirror JSON is kept as a file snapshot so a
 * converter change shows up as a reviewable diff.
 */
import { describe, expect, it } from 'vitest'
import { Node } from 'prosemirror-model'
import * as Y from 'yjs'
import { initProseMirrorDoc, updateYFragment, yXmlFragmentToProsemirrorJSON } from 'y-prosemirror'

import { schema } from './schema'

const fixtures = import.meta.glob('../../../../backend/tests/fixtures/docs_markdown/*.yjs.json', {
  eager: true,
  import: 'default',
}) as Record<string, { source: string; update: string }>

const decodeBase64 = (b64: string) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))

const load = (b64: string) => {
  const ydoc = new Y.Doc()
  Y.applyUpdate(ydoc, decodeBase64(b64))
  return { ydoc, fragment: ydoc.getXmlFragment('prosemirror') }
}

describe('docs_markdown golden fixtures', () => {
  const entries = Object.values(fixtures)

  it('finds the fixtures', () => {
    expect(entries.map((f) => f.source)).toEqual(
      expect.arrayContaining(['blocks.md', 'degrades.md', 'empty.md', 'getting-started.md', 'tables.md']),
    )
  })

  describe.each(entries.map((f) => [f.source, f] as const))('%s', (source, fixture) => {
    it('is valid under the editor schema', () => {
      const { fragment } = load(fixture.update)
      const doc = Node.fromJSON(schema, yXmlFragmentToProsemirrorJSON(fragment))
      expect(() => doc.check()).not.toThrow()
    })

    it('loads in the editor without y-prosemirror deleting anything', () => {
      const { ydoc, fragment } = load(fixture.update)
      const before = Y.encodeStateAsUpdate(ydoc)
      const { doc } = initProseMirrorDoc(fragment, schema)
      expect(() => doc.check()).not.toThrow()
      // A failed node build deletes its Yjs item, which changes the state.
      expect(Y.encodeStateAsUpdate(ydoc)).toEqual(before)
      expect(doc.toJSON()).toEqual(Node.fromJSON(schema, yXmlFragmentToProsemirrorJSON(fragment)).toJSON())
    })

    it('is not rewritten by the editor once loaded', () => {
      // y-prosemirror writes the editor's document back on the first local
      // transaction (a click is enough). An import the editor would write
      // differently is changed by whoever opens it first.
      const { ydoc, fragment } = load(fixture.update)
      const { doc, meta } = initProseMirrorDoc(fragment, schema)
      let changed = false
      ydoc.on('update', () => (changed = true))
      ydoc.transact(() => updateYFragment(ydoc, fragment, doc, meta as Parameters<typeof updateYFragment>[3]))
      expect(changed).toBe(false)
    })

    it('matches its snapshot', async () => {
      const { fragment } = load(fixture.update)
      const json = Node.fromJSON(schema, yXmlFragmentToProsemirrorJSON(fragment)).toJSON()
      const name = source.replace(/\.md$/, '')
      await expect(JSON.stringify(json, null, 2) + '\n').toMatchFileSnapshot(
        `./__snapshots__/docsMarkdownFixtures/${name}.json`,
      )
    })
  })

  it('would catch a document the editor cannot build', () => {
    // A list item must start with a paragraph; this one starts with code.
    const ydoc = new Y.Doc()
    const fragment = ydoc.getXmlFragment('prosemirror')
    const list = new Y.XmlElement('bullet_list')
    const item = new Y.XmlElement('list_item')
    item.insert(0, [new Y.XmlElement('code_block')])
    list.insert(0, [item])
    fragment.insert(0, [list])
    const doc = Node.fromJSON(schema, yXmlFragmentToProsemirrorJSON(fragment))
    expect(() => doc.check()).toThrow()
  })

  it('writes attrs with their JSON types and boolean marks as empty attrs', () => {
    const fixture = entries.find((f) => f.source === 'blocks.md')
    expect(fixture).toBeDefined()
    const { fragment } = load(fixture!.update)
    const heading = fragment.toArray().find((n) => n instanceof Y.XmlElement && n.nodeName === 'heading') as Y.XmlElement
    expect(typeof heading.getAttribute('level')).toBe('number')
    const json = yXmlFragmentToProsemirrorJSON(fragment)
    const marks = JSON.stringify(json).match(/"type":"strong","attrs":\{\}/g)
    expect(marks?.length).toBeGreaterThan(0)
  })
})
