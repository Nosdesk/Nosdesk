/**
 * Tables between collaborators: two or three editors bound to their own
 * Y.Docs, edited concurrently, then synced.
 *
 * The regression this pins (spike S2 in docs/plans/editor-tables-spikes.md):
 * prosemirror-tables repairs a ragged table in an appended transaction. When
 * that runs while y-prosemirror applies a remote change, the repair stays in
 * ProseMirror only, and each client writes its own copy of it on its next
 * keystroke anywhere, duplicating cells. A test that only syncs and compares
 * passes with or without the fix; the clients must type after the sync, here
 * in a paragraph outside the table, before syncing again.
 */
import { afterEach, describe, expect, it } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import type { Command, Plugin } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import * as Y from 'yjs'
import { ySyncPlugin, yXmlFragmentToProsemirrorJSON } from 'y-prosemirror'
import { CellSelection, TableMap, addColumnAfter, addRowAfter, tableEditing } from 'prosemirror-tables'
import { schema } from './schema'
import { initialEditorDoc } from './initialDoc'
import { createTableGuardPlugin } from './tableGuard'
import { allowTables, resetTablesAllowed } from './tablesAllowed'
import { cellSelectionRestore, collabTableEditing } from './tablePlugins'

const DOC_ID = 'ws-1_doc-1'

interface Client {
  ydoc: Y.Doc
  fragment: Y.XmlFragment
  view: EditorView
}

let clients: Client[] = []
afterEach(() => {
  clients.forEach((c) => c.view.destroy())
  clients = []
  resetTablesAllowed()
})

const p = (text = '') => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const td = (text: string) => schema.nodes.table_cell.create(null, p(text))

/** Intro, a 3x3 table with cells `r{row}c{col}`, and a paragraph below. */
function startDoc(): Node {
  const rows = [0, 1, 2].map((r) =>
    schema.nodes.table_row.create(null, [0, 1, 2].map((c) => td(`r${r}c${c}`))),
  )
  return schema.nodes.doc.create(null, [p('Intro'), schema.nodes.table.create(null, rows), p('Below')])
}

type TablePlugins = () => Plugin[]
const wrapped: TablePlugins = () => [cellSelectionRestore(), collabTableEditing()]
const unwrapped: TablePlugins = () => [tableEditing({ allowTableNodeSelection: true })]

function client(id: number, plugins: TablePlugins): Client {
  const ydoc = new Y.Doc({ gc: false })
  ydoc.clientID = id
  const fragment = ydoc.getXmlFragment('prosemirror')
  const { doc, mapping } = initialEditorDoc(fragment, schema)
  const view = new EditorView(document.createElement('div'), {
    state: EditorState.create({
      doc,
      schema,
      plugins: [ySyncPlugin(fragment, { mapping }), createTableGuardPlugin(() => DOC_ID), ...plugins()],
    }),
  })
  const c = { ydoc, fragment, view }
  clients.push(c)
  return c
}

function sync(...all: Client[]) {
  for (const from of all) {
    for (const to of all) {
      if (from === to) continue
      Y.applyUpdate(to.ydoc, Y.encodeStateAsUpdate(from.ydoc, Y.encodeStateVector(to.ydoc)))
    }
  }
}

/** `n` clients sharing the start document. */
function room(n: number, plugins: TablePlugins = wrapped): Client[] {
  allowTables(DOC_ID)
  const all = Array.from({ length: n }, (_, i) => client(i + 1, plugins))
  const first = all[0]!.view
  first.dispatch(first.state.tr.replaceWith(0, first.state.doc.content.size, startDoc().content))
  sync(...all)
  return all
}

/** Position of the first table, or -1. */
function tablePos(doc: Node): number {
  let pos = -1
  doc.forEach((node, offset) => {
    if (pos < 0 && node.type === schema.nodes.table) pos = offset
  })
  return pos
}

/** Inside the paragraph of cell (row, col). */
function inCell(view: EditorView, row: number, col: number): number {
  const at = tablePos(view.state.doc)
  const map = TableMap.get(view.state.doc.nodeAt(at)!)
  return at + 1 + map.map[row * map.width + col]! + 2
}

function cellPos(view: EditorView, row: number, col: number): number {
  return inCell(view, row, col) - 2
}

function run(view: EditorView, at: number, command: Command) {
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)))
  expect(command(view.state, view.dispatch)).toBe(true)
}

function typeAt(view: EditorView, at: number, text: string) {
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)))
  view.dispatch(view.state.tr.insertText(text))
}

/** Type at the end of the paragraph below the table. */
const typeBelow = (c: Client, text: string) => typeAt(c.view, c.view.state.doc.content.size - 1, text)

/** The shared document as this client's Yjs state holds it. */
const shared = (c: Client) => schema.nodeFromJSON(yXmlFragmentToProsemirrorJSON(c.fragment))

/** Cells per row of the first table. */
function widths(doc: Node): number[] {
  const out: number[] = []
  doc.nodeAt(tablePos(doc))!.forEach((row) => out.push(row.childCount))
  return out
}

/** Every client's editor matches its Yjs state, and all Yjs states agree. */
function expectConverged(all: Client[]) {
  const first = shared(all[0]!)
  for (const c of all) {
    expect(shared(c).eq(first)).toBe(true)
    expect(c.view.state.doc.eq(shared(c))).toBe(true)
    c.view.state.doc.check()
  }
}

describe('concurrent table edits', () => {
  it('row and column added at once stay free of duplicate cells when everyone types elsewhere', () => {
    const [a, b] = room(2)
    run(a!.view, inCell(a!.view, 1, 1), addRowAfter)
    run(b!.view, inCell(b!.view, 1, 1), addColumnAfter)
    sync(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!)).sort()).toEqual([3, 4, 4, 4])

    // The step that tells the wrapped plugin from the unwrapped one.
    typeBelow(a!, 'a')
    typeBelow(b!, 'b')
    sync(a!, b!)
    expectConverged([a!, b!])
    expect(Math.max(...widths(shared(a!)))).toBe(4)

    // The next local edit to the table repairs it, once, for everyone.
    typeAt(a!.view, inCell(a!.view, 0, 0), 'x')
    sync(a!, b!)
    typeBelow(b!, 'c')
    sync(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!))).toEqual([4, 4, 4, 4])
  })

  it('without the wrap, the same edits duplicate cells (the test can tell)', () => {
    const [a, b] = room(2, unwrapped)
    run(a!.view, inCell(a!.view, 1, 1), addRowAfter)
    run(b!.view, inCell(b!.view, 1, 1), addColumnAfter)
    sync(a!, b!)
    typeBelow(a!, 'a')
    typeBelow(b!, 'b')
    sync(a!, b!)
    expect(Math.max(...widths(shared(a!)))).toBeGreaterThan(4)
  })

  it('two columns added at once converge without ragged rows', () => {
    const [a, b] = room(2)
    run(a!.view, inCell(a!.view, 0, 2), addColumnAfter)
    run(b!.view, inCell(b!.view, 0, 0), addColumnAfter)
    sync(a!, b!)
    typeBelow(a!, 'a')
    typeBelow(b!, 'b')
    sync(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!))).toEqual([5, 5, 5])
  })

  it('three clients each adding a row converge with every row whole', () => {
    const all = room(3)
    all.forEach((c, i) => run(c.view, inCell(c.view, i, 0), addRowAfter))
    sync(...all)
    all.forEach((c, i) => typeBelow(c, String(i)))
    sync(...all)
    expectConverged(all)
    expect(widths(shared(all[0]!))).toEqual([3, 3, 3, 3, 3, 3])
  })
})

describe('a cell selection while a collaborator types', () => {
  function selectCells(view: EditorView, from: [number, number], to: [number, number]) {
    const sel = CellSelection.create(view.state.doc, cellPos(view, ...from), cellPos(view, ...to))
    view.dispatch(view.state.tr.setSelection(sel))
  }

  function selectedTexts(view: EditorView): string[] {
    const sel = view.state.selection
    expect(sel).toBeInstanceOf(CellSelection)
    const texts: string[] = []
    ;(sel as CellSelection).forEachCell((cell) => texts.push(cell.textContent))
    return texts
  }

  it('stays on the same cells when text changes elsewhere', () => {
    const [a, b] = room(2)
    selectCells(a!.view, [1, 0], [2, 1])
    typeBelow(b!, '!')
    typeAt(b!.view, inCell(b!.view, 0, 0), 'typed')
    sync(a!, b!)
    expect(selectedTexts(a!.view)).toEqual(['r1c0', 'r1c1', 'r2c0', 'r2c1'])
  })

  it('follows its cells when a row is added above them', () => {
    const [a, b] = room(2)
    selectCells(a!.view, [2, 2], [1, 1])
    run(b!.view, inCell(b!.view, 0, 0), addRowAfter)
    sync(a!, b!)
    const sel = a!.view.state.selection as CellSelection
    expect(selectedTexts(a!.view)).toEqual(['r1c1', 'r1c2', 'r2c1', 'r2c2'])
    expect(sel.$anchorCell.nodeAfter!.textContent).toBe('r2c2')
    expect(sel.$headCell.nodeAfter!.textContent).toBe('r1c1')
  })

  it('collapses without the restore plugin (the test can tell)', () => {
    const [a, b] = room(2, () => [collabTableEditing()])
    selectCells(a!.view, [1, 0], [2, 1])
    typeAt(b!.view, inCell(b!.view, 0, 0), 'typed')
    sync(a!, b!)
    expect(a!.view.state.selection).not.toBeInstanceOf(CellSelection)
  })
})
