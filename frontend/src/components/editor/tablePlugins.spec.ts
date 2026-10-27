import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { EditorState, NodeSelection, TextSelection } from 'prosemirror-state'
import type { Plugin } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import { keymap } from 'prosemirror-keymap'
import { baseKeymap, exitCode } from 'prosemirror-commands'
import { liftListItem, sinkListItem, splitListItem } from 'prosemirror-schema-list'
import { GapCursor } from 'prosemirror-gapcursor'
import { CellSelection, TableMap, columnResizingPluginKey } from 'prosemirror-tables'
import * as Y from 'yjs'
import { ySyncPlugin } from 'y-prosemirror'
import { schema } from './schema'
import { initialEditorDoc } from './initialDoc'
import { createTableGuardPlugin } from './tableGuard'
import { allowTables, resetTablesAllowed } from './tablesAllowed'
import { createTablePlugins, pipeRowCells } from './tablePlugins'

// The table plugins in the order CollaborativeEditor mounts them, around
// stand-ins for its own code-block and list keymaps.

const DOC_ID = 'ws-1_doc-1'

let views: EditorView[] = []
// Structure edits (rows, columns, cell attributes) carry table nodes, so the
// guard lets them through only where tables are allowed.
beforeEach(() => allowTables(DOC_ID))
afterEach(() => {
  views.forEach((v) => v.destroy())
  views = []
  resetTablesAllowed()
})

const p = (text = '') => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const td = (...blocks: Node[]) => schema.nodes.table_cell.create(null, blocks.length ? blocks : [p()])
const cell = (text: string) => td(p(text))
const th = (text: string) => schema.nodes.table_header.create(null, p(text))
const tr = (...cells: Node[]) => schema.nodes.table_row.create(null, cells)
const table = (...rows: Node[]) => schema.nodes.table.create(null, rows)
const doc = (...blocks: Node[]) => schema.nodes.doc.create(null, blocks)
const li = (text: string) => schema.nodes.list_item.create(null, p(text))
const ul = (...items: Node[]) => schema.nodes.bullet_list.create(null, items)
const code = (text: string) => schema.nodes.code_block.create(null, text ? schema.text(text) : null)

const grid = () => table(tr(th('Name'), th('Owner')), tr(cell('VPN'), cell('Ana')))

function open(start: Node, { resizable = false }: { resizable?: boolean } = {}): EditorView {
  const tables = createTablePlugins({ docId: () => DOC_ID, resizable })
  const plugins: Plugin[] = [
    createTableGuardPlugin(() => DOC_ID),
    keymap({ 'Mod-Enter': exitCode }),
    keymap({
      Enter: splitListItem(schema.nodes.list_item),
      Tab: sinkListItem(schema.nodes.list_item),
      'Shift-Tab': liftListItem(schema.nodes.list_item),
    }),
    tables.keymap,
    keymap(baseKeymap),
    ...tables.plugins,
  ]
  return mounted(EditorState.create({ doc: start, plugins }))
}

function mounted(state: EditorState): EditorView {
  const mount = document.createElement('div')
  document.body.appendChild(mount)
  const view = new EditorView(mount, { state })
  // jsdom does no layout; a caret at the edge of its textblock is at the
  // edge of the line for these tests.
  view.endOfTextblock = (dir) => {
    const { $head } = view.state.selection
    if (!$head.parent.inlineContent) return true
    return dir === 'left' || dir === 'up' || dir === 'backward'
      ? $head.parentOffset === 0
      : $head.parentOffset === $head.parent.content.size
  }
  views.push(view)
  return view
}

function press(view: EditorView, key: string, mods: { shift?: boolean; mod?: boolean } = {}) {
  const event = new KeyboardEvent('keydown', {
    key,
    shiftKey: mods.shift ?? false,
    ctrlKey: mods.mod ?? false,
    bubbles: true,
    cancelable: true,
  })
  view.dom.dispatchEvent(event)
  return event.defaultPrevented
}

/** Position inside the first paragraph of cell (row, col) of the first table. */
function inCell(view: EditorView, row: number, col: number, end = false): number {
  let tablePos = -1
  view.state.doc.forEach((node, offset) => {
    if (tablePos < 0 && node.type === schema.nodes.table) tablePos = offset
  })
  const map = TableMap.get(view.state.doc.nodeAt(tablePos)!)
  const cellPos = tablePos + 1 + map.map[row * map.width + col]!
  const start = cellPos + 2
  return end ? start + view.state.doc.nodeAt(cellPos)!.firstChild!.content.size : start
}

function caret(view: EditorView, pos: number) {
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)))
}

/** [row, col] of the cell holding the selection head. */
function where(view: EditorView): [number, number] {
  const { $head } = view.state.selection
  for (let d = $head.depth; d > 0; d--) {
    if ($head.node(d).type.spec.tableRole === 'table') {
      const map = TableMap.get($head.node(d))
      const rect = map.findCell($head.before(d + 2) - $head.start(d))
      return [rect.top, rect.left]
    }
  }
  return [-1, -1]
}

function rows(view: EditorView): string[][] {
  const out: string[][] = []
  view.state.doc.descendants((node) => {
    if (node.type === schema.nodes.table_row) {
      const cells: string[] = []
      node.forEach((c) => cells.push(c.textContent))
      out.push(cells)
      return false
    }
    return true
  })
  return out
}

describe('Tab and Shift-Tab', () => {
  it('move between cells and keep focus in the table at either end', () => {
    const view = open(doc(grid(), p()))
    caret(view, inCell(view, 0, 0))
    expect(press(view, 'Tab')).toBe(true)
    expect(where(view)).toEqual([0, 1])
    press(view, 'Tab')
    expect(where(view)).toEqual([1, 0])
    press(view, 'Tab', { shift: true })
    expect(where(view)).toEqual([0, 1])
    caret(view, inCell(view, 0, 0))
    expect(press(view, 'Tab', { shift: true })).toBe(true)
    expect(where(view)).toEqual([0, 0])
  })

  it('adds a row from the last cell and moves into it', () => {
    const view = open(doc(grid(), p()))
    caret(view, inCell(view, 1, 1))
    press(view, 'Tab')
    expect(rows(view)).toEqual([['Name', 'Owner'], ['VPN', 'Ana'], ['', '']])
    expect(where(view)).toEqual([2, 0])
  })

  it('indents a list item inside a cell before moving cells', () => {
    const view = open(doc(table(tr(td(ul(li('one'), li('two'))), cell('next'))), p()))
    const two = inCell(view, 0, 0) + 'one'.length + 6
    caret(view, two)
    expect(view.state.selection.$head.parent.textContent).toBe('two')
    press(view, 'Tab')
    const list = view.state.doc.child(0).child(0).child(0).firstChild!
    expect(list.childCount).toBe(1)
    expect(list.firstChild!.childCount).toBe(2)
    expect(where(view)).toEqual([0, 0])
  })

  it('leaves Tab alone outside tables', () => {
    const view = open(doc(p('Intro'), grid()))
    caret(view, 3)
    expect(press(view, 'Tab')).toBe(false)
  })
})

describe('Mod-Enter', () => {
  it('adds a row below the caret and moves into it', () => {
    const view = open(doc(grid(), p()))
    caret(view, inCell(view, 0, 1))
    press(view, 'Enter', { mod: true })
    expect(rows(view)).toEqual([['Name', 'Owner'], ['', ''], ['VPN', 'Ana']])
    expect(where(view)).toEqual([1, 0])
  })

  it('still exits a code block inside a cell', () => {
    const view = open(doc(table(tr(td(code('make dev')), cell('b'))), p()))
    caret(view, inCell(view, 0, 0, true))
    press(view, 'Enter', { mod: true })
    expect(rows(view)).toHaveLength(1)
    const first = view.state.doc.child(0).child(0).child(0)
    expect(first.childCount).toBe(2)
    expect(first.child(1).type).toBe(schema.nodes.paragraph)
    expect(view.state.selection.$head.parent.type).toBe(schema.nodes.paragraph)
  })
})

describe('arrows', () => {
  it('leave a table that ends the document with a gap cursor after it', () => {
    const view = open(doc(p('Intro'), grid()))
    caret(view, inCell(view, 1, 0, true))
    press(view, 'ArrowDown')
    expect(view.state.selection).toBeInstanceOf(GapCursor)
    expect(view.state.selection.head).toBe(view.state.doc.content.size)
  })

  it('leave a table that starts the document with a gap cursor before it', () => {
    const view = open(doc(grid(), p('After')))
    caret(view, inCell(view, 0, 1))
    press(view, 'ArrowUp')
    expect(view.state.selection).toBeInstanceOf(GapCursor)
    expect(view.state.selection.head).toBe(0)
  })

  it('cross from the last row into the paragraph below', () => {
    const view = open(doc(grid(), p('After')))
    caret(view, inCell(view, 1, 0, true))
    press(view, 'ArrowDown')
    expect(view.state.selection.$head.parent.textContent).toBe('After')
  })

  it('with Shift at a cell edge start a cell selection', () => {
    const view = open(doc(grid(), p()))
    caret(view, inCell(view, 0, 0, true))
    press(view, 'ArrowRight', { shift: true })
    const sel = view.state.selection
    expect(sel).toBeInstanceOf(CellSelection)
    expect((sel as CellSelection).ranges).toHaveLength(2)
  })
})

describe('Backspace and Delete', () => {
  it('clear the cells of a cell selection', () => {
    const view = open(doc(grid(), p()))
    const s = view.state
    const from = inCell(view, 1, 0) - 2
    const to = inCell(view, 1, 1) - 2
    view.dispatch(s.tr.setSelection(CellSelection.create(s.doc, from, to)))
    press(view, 'Backspace')
    expect(rows(view)).toEqual([['Name', 'Owner'], ['', '']])
  })

  it('delete a selected table', () => {
    const view = open(doc(p('Intro'), grid(), p('After')))
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, 7)))
    expect(view.state.selection).toBeInstanceOf(NodeSelection)
    press(view, 'Delete')
    expect(rows(view)).toEqual([])
    expect(view.state.doc.childCount).toBe(2)
  })
})

describe('the | shortcut', () => {
  const typed = (text: string) => doc(p('Intro'), p(text))

  it('turns a line of two or more cells into a table on Enter, caret in the new row', () => {
    const view = open(typed('| Name | Owner |'))
    caret(view, view.state.doc.content.size - 1)
    press(view, 'Enter')
    expect(rows(view)).toEqual([['Name', 'Owner'], ['', '']])
    expect(view.state.doc.child(1).child(0).child(0).type).toBe(schema.nodes.table_header)
    expect(view.state.doc.lastChild!.type).toBe(schema.nodes.paragraph)
    expect(where(view)).toEqual([1, 0])
  })

  it('keeps marks in the header cells', () => {
    const strong = schema.marks.strong.create()
    const line = schema.nodes.paragraph.create(null, [schema.text('| '), schema.text('Name', [strong]), schema.text(' | Owner |')])
    const cells = pipeRowCells(line)!
    expect(cells).toHaveLength(2)
    expect(cells[0]!.firstChild!.marks.map((m) => m.type.name)).toEqual(['strong'])
    expect(cells[1]!.textBetween(0, cells[1]!.size)).toBe('Owner')
  })

  it.each([
    ['one cell', '| Name |'],
    ['no closing pipe', '| Name | Owner'],
    ['no pipes', 'Name Owner'],
  ])('leaves %s as a paragraph', (_name, text) => {
    const view = open(typed(text))
    caret(view, view.state.doc.content.size - 1)
    press(view, 'Enter')
    expect(rows(view)).toEqual([])
  })

  it('does nothing until this document may hold tables', () => {
    resetTablesAllowed()
    const view = open(typed('| Name | Owner |'))
    caret(view, view.state.doc.content.size - 1)
    press(view, 'Enter')
    expect(rows(view)).toEqual([])
    expect(view.state.doc.childCount).toBe(3)
  })

  it('does nothing mid-line or inside a list', () => {
    const view = open(doc(p('| a | b |'), ul(li('| c | d |'))))
    caret(view, 3)
    press(view, 'Enter')
    caret(view, view.state.doc.content.size - 3)
    press(view, 'Enter')
    expect(rows(view)).toEqual([])
  })
})

describe('column resizing', () => {
  it('resets a column on a double-click on its border', () => {
    const sized = table(
      tr(
        schema.nodes.table_cell.create({ colwidth: [150] }, p('a')),
        schema.nodes.table_cell.create({ colwidth: [90] }, p('b')),
      ),
    )
    const view = open(doc(sized, p()), { resizable: true })
    expect(view.dom.querySelector('.table-scroll > table > colgroup')).not.toBeNull()
    const handle = inCell(view, 0, 0) - 2
    view.dispatch(view.state.tr.setMeta(columnResizingPluginKey, { setHandle: handle }))
    view.dom.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }))
    const first = view.state.doc.child(0).child(0)
    expect(first.child(0).attrs.colwidth).toBeNull()
    expect(first.child(1).attrs.colwidth).toEqual([90])
  })

  it('is off without a fine pointer', () => {
    const view = open(doc(grid(), p()))
    expect(columnResizingPluginKey.getState(view.state)).toBeUndefined()
  })
})

describe('the gap cursor on a new shared document', () => {
  // It was once removed for throwing on empty Yjs documents; an editor now
  // starts from one empty paragraph (initialEditorDoc).
  it('takes arrows, typing and a table insert without throwing', () => {
    const fragment = new Y.Doc().getXmlFragment('prosemirror')
    const { doc: start, mapping } = initialEditorDoc(fragment, schema)
    const tables = createTablePlugins({ docId: () => DOC_ID, resizable: false })
    const view = mounted(
      EditorState.create({
        doc: start,
        schema,
        plugins: [ySyncPlugin(fragment, { mapping }), tables.keymap, keymap(baseKeymap), ...tables.plugins],
      }),
    )
    for (const key of ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']) press(view, key)
    view.dispatch(view.state.tr.insertText('| a | b |'))
    press(view, 'Enter')
    press(view, 'ArrowUp')
    press(view, 'ArrowUp')
    expect(view.state.selection).toBeInstanceOf(GapCursor)
    view.dispatch(view.state.tr.insertText('x'))
    expect(view.state.doc.firstChild!.textContent).toBe('x')
    expect(fragment.length).toBe(3)
  })
})
