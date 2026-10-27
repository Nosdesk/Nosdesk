import { describe, expect, it, vi } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import type { Command } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import { CellSelection, TableMap } from 'prosemirror-tables'
import { schema } from './schema'
import { selectAllCells, selectColumn, selectRow } from './tableCommands'
import { columnMenu, groupedMenu, rowMenu, selectionMenu, tableMenu, tableUiState } from './tableMenus'
import type { TableMenuContext, TableMenuItem } from './tableMenus'

const p = (text = '') => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const td = (text: string, attrs?: Record<string, unknown>) => schema.nodes.table_cell.create(attrs, p(text))
const th = (text: string) => schema.nodes.table_header.create(null, p(text))
const tr = (...cells: Node[]) => schema.nodes.table_row.create(null, cells)
const table = (...rows: Node[]) => schema.nodes.table.create(null, rows)
const doc = (...blocks: Node[]) => schema.nodes.doc.create(null, blocks)

/** Header row over three body rows, three columns. Table at position 3. */
const grid = () =>
  table(
    tr(th('Name'), th('Owner'), th('Notes')),
    tr(td('VPN'), td('Ana'), td('a')),
    tr(td('Mail'), td('Bo'), td('b')),
    tr(td('Wiki'), td('Cy'), td('c')),
  )
const TABLE_POS = 3

function caretIn(d: Node, row: number, col: number): EditorState {
  const s = EditorState.create({ doc: d })
  const tableNode = s.doc.nodeAt(TABLE_POS)!
  const map = TableMap.get(tableNode)
  const pos = TABLE_POS + 1 + map.map[row * map.width + col]! + 2
  return s.apply(s.tr.setSelection(TextSelection.create(s.doc, pos)))
}

function apply(state: EditorState, command: Command): EditorState {
  let next = state
  command(state, (t) => (next = state.apply(t)))
  return next
}

const context: TableMenuContext = {
  t: (key, args) => (args ? `${key}:${Object.values(args).join(',')}` : key),
  modKey: 'Ctrl',
  deleteTable: vi.fn(),
  copyMarkdown: vi.fn(),
}

const ids = (items: TableMenuItem[]) => items.map((i) => i.id)
const item = (items: TableMenuItem[], id: string) => items.find((i) => i.id === id)!

/** A stand-in view whose dispatch applies to its own state. */
function fakeView(state: EditorState): EditorView {
  const view = {
    state,
    dispatch: (t: Parameters<EditorView['dispatch']>[0]) => {
      view.state = view.state.apply(t)
    },
    focus: () => {},
  }
  return view as unknown as EditorView
}

const texts = (state: EditorState) => {
  const out: string[][] = []
  state.doc.nodeAt(TABLE_POS)!.forEach((row) => {
    const cells: string[] = []
    row.forEach((c) => cells.push((c.type === schema.nodes.table_header ? '#' : '') + c.textContent))
    out.push(cells)
  })
  return out
}

describe('tableUiState', () => {
  it('is null outside a table', () => {
    expect(tableUiState(EditorState.create({ doc: doc(p('x'), grid()) }))).toBeNull()
  })

  it('gives the caret row and column, nothing selected', () => {
    expect(tableUiState(caretIn(doc(p('x'), grid()), 2, 1))).toEqual({
      tablePos: TABLE_POS,
      row: 2,
      col: 1,
      rows: [],
      cols: [],
      all: false,
      cells: false,
    })
  })

  it('lists selected rows and columns, and the whole table', () => {
    const start = caretIn(doc(p('x'), grid()), 0, 0)
    const rows = tableUiState(apply(start, selectRow(TABLE_POS, 1, 2)))!
    expect(rows.rows).toEqual([1, 2])
    expect(rows.cols).toEqual([])
    expect(rows.cells).toBe(true)
    const col = tableUiState(apply(start, selectColumn(TABLE_POS, 2)))!
    expect(col.cols).toEqual([2])
    const all = tableUiState(apply(start, selectAllCells(TABLE_POS)))!
    expect(all.all).toBe(true)
    expect(all.rows).toEqual([0, 1, 2, 3])
  })

  it('counts a single merged cell as a cell selection', () => {
    const merged = table(tr(th('A'), th('B')), tr(td('wide', { colspan: 2 })))
    const s = EditorState.create({ doc: doc(p('x'), merged) })
    const cellPos = TABLE_POS + 1 + TableMap.get(merged).map[2]!
    const selected = s.apply(s.tr.setSelection(CellSelection.create(s.doc, cellPos)))
    expect(tableUiState(selected)!.cells).toBe(true)
  })
})

describe('rowMenu', () => {
  const state = caretIn(doc(p('x'), grid()), 1, 1)

  it('offers Header row on the first row only', () => {
    expect(ids(rowMenu(state, 0, context))).toContain('row-header')
    expect(item(rowMenu(state, 0, context), 'row-header').checked).toBe(true)
    expect(ids(rowMenu(state, 2, context))).not.toContain('row-header')
  })

  it('greys out moves past the edges and across the header', () => {
    const disabled = (r: number) =>
      rowMenu(state, r, context)
        .filter((i) => i.disabled)
        .map((i) => i.id)
    expect(disabled(0)).toEqual(['row-up', 'row-down'])
    expect(disabled(1)).toEqual(['row-up'])
    expect(disabled(2)).toEqual([])
    expect(disabled(3)).toEqual(['row-down'])
  })

  it('lets the first body row move up when there is no header', () => {
    const plain = table(tr(td('a')), tr(td('b')))
    const s = caretIn(doc(p('x'), plain), 1, 0)
    expect(item(rowMenu(s, 1, context), 'row-up').disabled).toBe(false)
    expect(item(rowMenu(s, 0, context), 'row-header').checked).toBe(false)
  })

  it('shows the add-row-below shortcut', () => {
    expect(item(rowMenu(state, 1, context), 'row-below').trailing).toBe('Ctrl↩')
  })

  it('acts on its own row wherever the caret is', () => {
    const view = fakeView(state)
    item(rowMenu(state, 3, context), 'row-delete').run!(view)
    expect(texts(view.state).map((r) => r[0])).toEqual(['#Name', 'VPN', 'Mail'])
    const moved = fakeView(state)
    item(rowMenu(state, 2, context), 'row-up').run!(moved)
    expect(texts(moved.state).map((r) => r[0])).toEqual(['#Name', 'Mail', 'VPN', 'Wiki'])
    const cleared = fakeView(state)
    item(rowMenu(state, 2, context), 'row-clear').run!(cleared)
    expect(texts(cleared.state)[2]).toEqual(['', '', ''])
    expect(texts(cleared.state)[1]).toEqual(['VPN', 'Ana', 'a'])
  })

  it('promotes a row inserted above the header', () => {
    const view = fakeView(state)
    item(rowMenu(state, 0, context), 'row-above').run!(view)
    expect(texts(view.state).slice(0, 2)).toEqual([
      ['#', '#', '#'],
      ['Name', 'Owner', 'Notes'],
    ])
  })
})

describe('columnMenu', () => {
  const state = caretIn(doc(p('x'), grid()), 1, 1)

  it('offers Header column on the first column only', () => {
    expect(ids(columnMenu(state, 0, context))).toContain('col-header')
    expect(item(columnMenu(state, 0, context), 'col-header').checked).toBe(false)
    expect(ids(columnMenu(state, 1, context))).not.toContain('col-header')
  })

  it('greys out moves past the edges', () => {
    const disabled = (c: number) =>
      columnMenu(state, c, context)
        .filter((i) => i.disabled)
        .map((i) => i.id)
    expect(disabled(0)).toEqual(['col-move-left'])
    expect(disabled(1)).toEqual([])
    expect(disabled(2)).toEqual(['col-move-right'])
  })

  it('keeps a header column in place', () => {
    const crossed = table(tr(th(''), th('A')), tr(th('x'), td('1')))
    const s = caretIn(doc(p('x'), crossed), 1, 1)
    expect(item(columnMenu(s, 1, context), 'col-move-left').disabled).toBe(true)
    expect(item(columnMenu(s, 0, context), 'col-move-right').disabled).toBe(true)
  })

  it('offers Reset width only on a resized column', () => {
    expect(ids(columnMenu(state, 1, context))).not.toContain('col-reset')
    const sized = table(tr(th('A'), schema.nodes.table_header.create({ colwidth: [140] }, p('B'))), tr(td('1'), td('2')))
    const s = caretIn(doc(p('x'), sized), 1, 0)
    expect(ids(columnMenu(s, 1, context))).toContain('col-reset')
    expect(ids(columnMenu(s, 0, context))).not.toContain('col-reset')
  })

  it('ticks the column alignment and aligns its own column', () => {
    const items = columnMenu(state, 2, context)
    expect(item(items, 'col-align-left').checked).toBe(true)
    const view = fakeView(state)
    item(items, 'col-align-center').run!(view)
    const after = view.state.doc.nodeAt(TABLE_POS)!
    expect(after.child(1).child(2).attrs.align).toBe('center')
    expect(after.child(1).child(1).attrs.align).toBeNull()
    expect(item(columnMenu(view.state, 2, context), 'col-align-center').checked).toBe(true)
  })
})

describe('tableMenu', () => {
  it('ticks the headers, greys out Reset column widths when nothing is resized', () => {
    const items = tableMenu(caretIn(doc(p('x'), grid()), 1, 1), context)
    expect(ids(items)).toEqual(['table-header-row', 'table-header-column', 'table-reset-widths', 'table-copy-markdown', 'table-delete'])
    expect(item(items, 'table-header-row').checked).toBe(true)
    expect(item(items, 'table-header-column').checked).toBe(false)
    expect(item(items, 'table-reset-widths').disabled).toBe(true)
    expect(item(items, 'table-delete').trailing).toBeUndefined()
  })

  it('shows Backspace on Delete table while every cell is selected', () => {
    const all = apply(caretIn(doc(p('x'), grid()), 1, 1), selectAllCells(TABLE_POS))
    expect(item(tableMenu(all, context), 'table-delete').trailing).toBe('⌫')
  })

  it('hands delete and copy to the editor', () => {
    const state = caretIn(doc(p('x'), grid()), 1, 1)
    const view = fakeView(state)
    item(tableMenu(state, context), 'table-delete').run!(view)
    item(tableMenu(state, context), 'table-copy-markdown').run!(view)
    expect(context.deleteTable).toHaveBeenCalledWith(view)
    expect(context.copyMarkdown).toHaveBeenCalledWith(view)
  })
})

describe('selectionMenu', () => {
  it('is empty for a caret or a single plain cell', () => {
    const state = caretIn(doc(p('x'), grid()), 1, 1)
    expect(selectionMenu(state, context)).toEqual([])
    const one = state.apply(state.tr.setSelection(CellSelection.create(state.doc, TABLE_POS + 1 + TableMap.get(grid()).map[4]!)))
    expect(selectionMenu(one, context)).toEqual([])
  })

  it('offers merge for several cells and split for a merged one', () => {
    const several = apply(caretIn(doc(p('x'), grid()), 1, 1), selectRow(TABLE_POS, 1))
    expect(ids(selectionMenu(several, context))).toEqual([
      'cells-merge',
      'cells-align-left',
      'cells-align-center',
      'cells-align-right',
      'cells-clear',
    ])
    const merged = table(tr(th('A'), th('B')), tr(td('wide', { colspan: 2 })))
    const s = EditorState.create({ doc: doc(p('x'), merged) })
    const one = s.apply(s.tr.setSelection(CellSelection.create(s.doc, TABLE_POS + 1 + TableMap.get(merged).map[2]!)))
    expect(ids(selectionMenu(one, context))).toEqual([
      'cells-split',
      'cells-align-left',
      'cells-align-center',
      'cells-align-right',
      'cells-clear',
    ])
  })
})

describe('groupedMenu', () => {
  it('groups the caret row, its column and the table under headings', () => {
    const items = groupedMenu(caretIn(doc(p('x'), grid()), 2, 1), context)
    const headings = items.filter((i) => i.heading).map((i) => i.label)
    expect(headings).toEqual(['editor-table-menu-row:3', 'editor-table-menu-column:2', 'editor-table-menu-table'])
    expect(items.filter((i) => i.divider).map((i) => i.id)).toEqual(['heading-column', 'heading-table'])
  })

  it('puts the selected cells first', () => {
    const several = apply(caretIn(doc(p('x'), grid()), 1, 1), selectRow(TABLE_POS, 1))
    const items = groupedMenu(several, context)
    expect(items[0]!.id).toBe('heading-cells')
    expect(item(items, 'heading-row').divider).toBe(true)
  })

  it('is empty outside a table', () => {
    expect(groupedMenu(EditorState.create({ doc: doc(p('x'), grid()) }), context)).toEqual([])
  })
})
