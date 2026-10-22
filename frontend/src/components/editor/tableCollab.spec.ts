/**
 * Tables between collaborators: two or three editors bound to their own
 * Y.Docs, edited concurrently, then synced.
 *
 * The regressions this pins:
 * - Spike S2 in docs/plans/editor-tables-spikes.md: prosemirror-tables
 *   repairs a ragged table in an appended transaction. When that runs while
 *   y-prosemirror applies a remote change, the repair stays in ProseMirror
 *   only, and each client writes its own copy of it on its next keystroke
 *   anywhere, duplicating cells. A test that only syncs and compares passes
 *   with or without the fix; the clients must type after the sync, here in a
 *   paragraph outside the table, before syncing again.
 * - A row and a column added at once left the new row a cell short in every
 *   window until a later structural edit. Now the row's creator pads it as
 *   soon as the change arrives, and no one else does, so two clients never
 *   both add the cell.
 */
import { afterEach, describe, expect, it } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import type { Command, Plugin } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import * as Y from 'yjs'
import { ySyncPlugin, yXmlFragmentToProsemirrorJSON } from 'y-prosemirror'
import { CellSelection, TableMap, addColumnAfter, addRowAfter, fixTablesKey, tableEditing } from 'prosemirror-tables'
import { schema } from './schema'
import { initialEditorDoc } from './initialDoc'
import { createTableGuardPlugin } from './tableGuard'
import { allowTables, resetTablesAllowed } from './tablesAllowed'
import { addRowBelowAndEnter } from './tableCommands'
import { cellSelectionRestore, collabTableEditing, tableRepair } from './tablePlugins'

const DOC_ID = 'ws-1_doc-1'

interface Client {
  ydoc: Y.Doc
  fragment: Y.XmlFragment
  view: EditorView
  /** Table repairs this client has dispatched. */
  repairs: number
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
const wrapped: TablePlugins = () => [cellSelectionRestore(), collabTableEditing(), tableRepair()]
const unwrapped: TablePlugins = () => [tableEditing({ allowTableNodeSelection: true })]

function client(id: number, plugins: TablePlugins): Client {
  const ydoc = new Y.Doc({ gc: false })
  ydoc.clientID = id
  const fragment = ydoc.getXmlFragment('prosemirror')
  const { doc, mapping } = initialEditorDoc(fragment, schema)
  const c: Client = { ydoc, fragment, view: null as unknown as EditorView, repairs: 0 }
  const view = new EditorView(document.createElement('div'), {
    state: EditorState.create({
      doc,
      schema,
      plugins: [ySyncPlugin(fragment, { mapping }), createTableGuardPlugin(() => DOC_ID), ...plugins()],
    }),
    dispatchTransaction(tr) {
      if (tr.getMeta(fixTablesKey)) c.repairs++
      view.updateState(view.state.apply(tr))
    },
  })
  c.view = view
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

/** Let the clients' deferred table repairs run. */
const settle = () => new Promise((r) => setTimeout(r, 0))

/** Sync, let the repairs run, and sync them. */
async function syncSettled(...all: Client[]) {
  sync(...all)
  await settle()
  sync(...all)
  await settle()
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
  it('row and column added at once: the row is padded once, by its creator, for everyone', async () => {
    const [a, b] = room(2)
    run(a!.view, inCell(a!.view, 1, 1), addRowBelowAndEnter)
    run(b!.view, inCell(b!.view, 1, 1), addColumnAfter)
    sync(a!, b!)
    // Merged, the new row is short of the new column's cell on both sides.
    expect(widths(a!.view.state.doc).sort()).toEqual([3, 4, 4, 4])
    expect(widths(b!.view.state.doc).sort()).toEqual([3, 4, 4, 4])

    await settle()
    expect(widths(a!.view.state.doc)).toEqual([4, 4, 4, 4])
    expect([a!.repairs, b!.repairs]).toEqual([1, 0])
    // The repair reached Yjs rather than staying in ProseMirror.
    expect(a!.view.state.doc.eq(shared(a!))).toBe(true)

    await syncSettled(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!))).toEqual([4, 4, 4, 4])
    expect([a!.repairs, b!.repairs]).toEqual([1, 0])

    // Typing anywhere afterwards adds nothing.
    typeBelow(a!, 'a')
    typeBelow(b!, 'b')
    typeAt(a!.view, inCell(a!.view, 3, 3), 'x')
    typeAt(b!.view, inCell(b!.view, 0, 0), 'y')
    await syncSettled(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!))).toEqual([4, 4, 4, 4])
  })

  it('the other client typing in the ragged table before the repair arrives adds no second cell', async () => {
    const [a, b] = room(2)
    run(a!.view, inCell(a!.view, 1, 1), addRowAfter)
    run(b!.view, inCell(b!.view, 1, 1), addColumnAfter)
    sync(a!, b!)
    // B edits the table at once, in its own new column and in the short row.
    typeAt(b!.view, inCell(b!.view, 0, 2), 'new')
    typeAt(b!.view, inCell(b!.view, 2, 0), 'short')
    await settle()
    expect(b!.repairs).toBe(0)
    await syncSettled(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!))).toEqual([4, 4, 4, 4])
    expect(shared(a!).textContent).toContain('new')
    expect(shared(a!).textContent).toContain('short')
  })

  it('a client that only watched does not repair', async () => {
    const [a, b, c] = room(3)
    run(a!.view, inCell(a!.view, 1, 1), addRowAfter)
    run(b!.view, inCell(b!.view, 1, 1), addColumnAfter)
    await syncSettled(a!, b!, c!)
    expectConverged([a!, b!, c!])
    expect(widths(shared(c!))).toEqual([4, 4, 4, 4])
    expect([a!.repairs, b!.repairs, c!.repairs]).toEqual([1, 0, 0])
  })

  it('when the row creator has left, the present client with the lowest id repairs', async () => {
    const present = () => [2, 3]
    const all = room(3, () => [cellSelectionRestore(), collabTableEditing(), tableRepair({ present })])
    const [a, b, c] = all
    run(a!.view, inCell(a!.view, 1, 1), addRowAfter)
    run(b!.view, inCell(b!.view, 1, 1), addColumnAfter)
    // A's row reaches the others, then A leaves before it sees B's column.
    sync(a!, c!)
    sync(a!, b!)
    a!.view.destroy()
    clients = [b!, c!]
    await syncSettled(b!, c!)
    expectConverged([b!, c!])
    expect(widths(shared(b!))).toEqual([4, 4, 4, 4])
    expect([b!.repairs, c!.repairs]).toEqual([1, 0])
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

  it('two columns added at once converge without ragged rows', async () => {
    const [a, b] = room(2)
    run(a!.view, inCell(a!.view, 0, 2), addColumnAfter)
    run(b!.view, inCell(b!.view, 0, 0), addColumnAfter)
    await syncSettled(a!, b!)
    typeBelow(a!, 'a')
    typeBelow(b!, 'b')
    await syncSettled(a!, b!)
    expectConverged([a!, b!])
    expect(widths(shared(a!))).toEqual([5, 5, 5])
    expect([a!.repairs, b!.repairs]).toEqual([0, 0])
  })

  it('three clients each adding a row converge with every row whole', async () => {
    const all = room(3)
    all.forEach((c, i) => run(c.view, inCell(c.view, i, 0), addRowAfter))
    await syncSettled(...all)
    all.forEach((c, i) => typeBelow(c, String(i)))
    await syncSettled(...all)
    expectConverged(all)
    expect(widths(shared(all[0]!))).toEqual([3, 3, 3, 3, 3, 3])
  })

  it('two rows and a column added at once: each row creator pads its own row', async () => {
    const [a, b, c] = room(3)
    run(a!.view, inCell(a!.view, 0, 0), addRowAfter)
    run(b!.view, inCell(b!.view, 2, 0), addRowAfter)
    run(c!.view, inCell(c!.view, 1, 1), addColumnAfter)
    await syncSettled(a!, b!, c!)
    expectConverged([a!, b!, c!])
    expect(widths(shared(a!))).toEqual([4, 4, 4, 4, 4])
    expect([a!.repairs, b!.repairs, c!.repairs]).toEqual([1, 1, 0])
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
