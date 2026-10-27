import { describe, expect, it } from 'vitest'
import { EditorState, NodeSelection, TextSelection } from 'prosemirror-state'
import type { Command } from 'prosemirror-state'
import type { Node } from 'prosemirror-model'
import { CellSelection, TableMap } from 'prosemirror-tables'
import { schema } from './schema'
import {
  addColumnAfter,
  addColumnBefore,
  addRowAfter,
  addRowBefore,
  alignColumn,
  alignSelection,
  columnHasWidth,
  deleteSelectedCellsTable,
  insertColumnAt,
  insertRowAt,
  selectAllCells,
  selectAllInTable,
  selectColumn,
  selectRow,
  tableToMarkdown,
  clearCells,
  deleteColumn,
  deleteRow,
  deleteTable,
  insertTable,
  mergeCells,
  moveColumnLeft,
  moveColumnRight,
  moveRowDown,
  moveRowUp,
  resetColumnWidth,
  resetColumnWidthAtHandle,
  resetColumnWidths,
  splitCell,
  tableHasWidths,
  toggleHeaderColumn,
  toggleHeaderRow,
} from './tableCommands'

const p = (text = '') => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const td = (text: string, attrs?: Record<string, unknown>) => schema.nodes.table_cell.create(attrs, p(text))
const th = (text: string) => schema.nodes.table_header.create(null, p(text))
const tr = (...cells: Node[]) => schema.nodes.table_row.create(null, cells)
const table = (...rows: Node[]) => schema.nodes.table.create(null, rows)
const doc = (...blocks: Node[]) => schema.nodes.doc.create(null, blocks)

/** 2x2 body under a header row: Name/Owner, VPN/Ana, Mail/Bo. */
const grid = () => table(tr(th('Name'), th('Owner')), tr(td('VPN'), td('Ana')), tr(td('Mail'), td('Bo')))

/** The position inside the paragraph of cell (row, col) of the doc's first table. */
function cellText(state: EditorState, row: number, col: number): number {
  let tablePos = -1
  state.doc.forEach((node, offset) => {
    if (tablePos < 0 && node.type === schema.nodes.table) tablePos = offset
  })
  const tableNode = state.doc.nodeAt(tablePos)!
  const map = TableMap.get(tableNode)
  return tablePos + 1 + map.map[row * map.width + col]! + 2
}

function stateWith(d: Node, at: (s: EditorState) => number | [number, number] = () => 1): EditorState {
  const s = EditorState.create({ doc: d })
  const where = at(s)
  if (Array.isArray(where)) {
    const [anchor, head] = where
    return s.apply(s.tr.setSelection(CellSelection.create(s.doc, anchor, head)))
  }
  return s.apply(s.tr.setSelection(TextSelection.create(s.doc, where)))
}

function run(state: EditorState, command: Command): EditorState {
  let next = state
  const applied = command(state, (t) => {
    next = state.apply(t)
  })
  expect(applied).toBe(true)
  next.doc.check()
  return next
}

/** Each row's cell texts, header cells marked with `#`. */
function rows(state: EditorState): string[][] {
  const out: string[][] = []
  state.doc.descendants((node) => {
    if (node.type === schema.nodes.table_row) {
      const cells: string[] = []
      node.forEach((c) => cells.push((c.type === schema.nodes.table_header ? '#' : '') + c.textContent))
      out.push(cells)
      return false
    }
    return true
  })
  return out
}

const cellAt = (state: EditorState, row: number, col: number) => state.doc.resolve(cellText(state, row, col) - 2)
const cellPos = (state: EditorState, row: number, col: number) => cellText(state, row, col) - 2

describe('insertTable', () => {
  it('adds a header row and two rows of three columns after the current block, caret in the first header', () => {
    const s = run(stateWith(doc(p('Intro'), p('After'))), insertTable)
    expect(s.doc.child(1).type).toBe(schema.nodes.table)
    expect(rows(s)).toEqual([
      ['#', '#', '#'],
      ['', '', ''],
      ['', '', ''],
    ])
    expect(s.doc.child(0).textContent).toBe('Intro')
    expect(s.doc.child(2).textContent).toBe('After')
    expect(s.selection.$from.node(-1).type).toBe(schema.nodes.table_header)
    expect(s.selection.from).toBe(cellText(s, 0, 0))
  })

  it('keeps a paragraph after a table that would end the document', () => {
    const s = run(stateWith(doc(p('Intro'))), insertTable)
    expect(s.doc.childCount).toBe(3)
    expect(s.doc.lastChild!.type).toBe(schema.nodes.paragraph)
  })

  it('takes the place of an empty paragraph', () => {
    const s = run(stateWith(doc(p('Intro'), p()), () => 8), insertTable)
    expect(s.doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual(['paragraph', 'table', 'paragraph'])
  })

  it('goes after a table when the caret is in one', () => {
    const s = run(stateWith(doc(grid(), p('After')), (st) => cellText(st, 1, 1)), insertTable)
    expect(s.doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual(['table', 'table', 'paragraph'])
  })
})

describe('row and column commands', () => {
  const inCell = (row: number, col: number) => stateWith(doc(grid(), p()), (s) => cellText(s, row, col))

  it('adds rows above and below', () => {
    expect(rows(run(inCell(1, 0), addRowBefore))).toHaveLength(4)
    const below = run(inCell(1, 0), addRowAfter)
    expect(rows(below)[2]).toEqual(['', ''])
  })

  it('adds columns left and right', () => {
    expect(rows(run(inCell(1, 0), addColumnBefore))[1]).toEqual(['', 'VPN', 'Ana'])
    expect(rows(run(inCell(1, 0), addColumnAfter))[1]).toEqual(['VPN', '', 'Ana'])
  })

  it('deletes a row and a column', () => {
    expect(rows(run(inCell(1, 0), deleteRow))).toEqual([['#Name', '#Owner'], ['Mail', 'Bo']])
    expect(rows(run(inCell(1, 0), deleteColumn))).toEqual([['#Owner'], ['Ana'], ['Bo']])
  })

  it('moves rows up and down, and not past the edges', () => {
    expect(rows(run(inCell(2, 0), moveRowUp)).map((r) => r[0])).toEqual(['#Name', 'Mail', 'VPN'])
    expect(rows(run(inCell(1, 0), moveRowDown)).map((r) => r[0])).toEqual(['#Name', 'Mail', 'VPN'])
    expect(moveRowDown(inCell(2, 0))).toBe(false)
    expect(moveRowUp(inCell(0, 0))).toBe(false)
  })

  it('moves columns left and right, and not past the edges', () => {
    expect(rows(run(inCell(1, 1), moveColumnLeft))[1]).toEqual(['Ana', 'VPN'])
    expect(rows(run(inCell(1, 0), moveColumnRight))[1]).toEqual(['Ana', 'VPN'])
    expect(moveColumnLeft(inCell(1, 0))).toBe(false)
    expect(moveColumnRight(inCell(1, 1))).toBe(false)
  })

  it('toggles the header row and the header column wherever the caret is', () => {
    const noHeader = run(inCell(2, 1), toggleHeaderRow)
    expect(rows(noHeader)[0]).toEqual(['Name', 'Owner'])
    expect(rows(run(noHeader, toggleHeaderRow))[0]).toEqual(['#Name', '#Owner'])
    const column = run(inCell(2, 1), toggleHeaderColumn)
    expect(rows(column).map((r) => r[0])).toEqual(['#Name', '#VPN', '#Mail'])
  })

  it('merges a cell selection and splits it again', () => {
    const selected = stateWith(doc(grid(), p()), (s) => [cellPos(s, 1, 0), cellPos(s, 1, 1)])
    const merged = run(selected, mergeCells)
    expect(rows(merged)[1]).toHaveLength(1)
    expect(merged.doc.child(0).child(1).child(0).attrs.colspan).toBe(2)
    const caretIn = merged.apply(merged.tr.setSelection(TextSelection.create(merged.doc, cellText(merged, 1, 0))))
    expect(rows(run(caretIn, splitCell))[1]).toHaveLength(2)
  })

  it('aligns whole columns, header included, and clears the alignment', () => {
    const centred = run(inCell(2, 1), alignColumn('center'))
    const aligns = (s: EditorState) => [0, 1, 2].map((r) => cellAt(s, r, 1).nodeAfter!.attrs.align)
    expect(aligns(centred)).toEqual(['center', 'center', 'center'])
    expect(cellAt(centred, 1, 0).nodeAfter!.attrs.align).toBeNull()
    expect(aligns(run(centred, alignColumn(null)))).toEqual([null, null, null])
  })

  it('clears selected cells, or the cell with the caret', () => {
    const selected = stateWith(doc(grid(), p()), (s) => [cellPos(s, 1, 0), cellPos(s, 2, 0)])
    expect(rows(run(selected, clearCells)).map((r) => r[0])).toEqual(['#Name', '', ''])
    expect(rows(run(inCell(1, 1), clearCells))[1]).toEqual(['VPN', ''])
  })
})

describe('column widths', () => {
  const sized = () =>
    stateWith(
      doc(table(tr(td('a', { colwidth: [120] }), td('b', { colwidth: [200] })), tr(td('c', { colwidth: [120] }), td('d', { colwidth: [200] }))), p()),
      (s) => cellText(s, 1, 0),
    )
  const widths = (s: EditorState) =>
    [0, 1].map((r) => [0, 1].map((c) => cellAt(s, r, c).nodeAfter!.attrs.colwidth as number[] | null))

  it('resets the column with the caret', () => {
    const s = run(sized(), resetColumnWidth)
    expect(widths(s)).toEqual([
      [null, [200]],
      [null, [200]],
    ])
    expect(tableHasWidths(s)).toBe(true)
  })

  it('reports nothing to reset when the column has no width', () => {
    const s = run(sized(), resetColumnWidth)
    expect(resetColumnWidth(s)).toBe(false)
  })

  it('resets every column', () => {
    const s = run(sized(), resetColumnWidths)
    expect(tableHasWidths(s)).toBe(false)
    expect(resetColumnWidths(s)).toBe(false)
  })

  it('resets the column a resize handle sits on', () => {
    const s = sized()
    const t = resetColumnWidthAtHandle(s, cellPos(s, 0, 1))!
    expect(widths(s.apply(t))).toEqual([
      [[120], null],
      [[120], null],
    ])
  })

  it('resets one column of a merged cell, keeping the other', () => {
    const s = stateWith(
      doc(table(tr(td('wide', { colspan: 2, colwidth: [100, 150] })), tr(td('x'), td('y'))), p()),
      (st) => cellText(st, 1, 1),
    )
    const next = run(s, resetColumnWidth)
    expect(next.doc.child(0).child(0).child(0).attrs.colwidth).toEqual([100, 0])
  })
})

describe('deleteTable', () => {
  it('deletes the table holding the caret', () => {
    const s = run(stateWith(doc(p('Intro'), grid(), p('After')), (st) => cellText(st, 1, 1)), deleteTable)
    expect(s.doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual(['paragraph', 'paragraph'])
  })

  it('deletes a selected table node', () => {
    const start = EditorState.create({ doc: doc(p('Intro'), grid(), p('After')) })
    const s = start.apply(start.tr.setSelection(NodeSelection.create(start.doc, 7)))
    expect(run(s, deleteTable).doc.childCount).toBe(2)
  })

  it('leaves an empty paragraph when the table was the whole document', () => {
    const s = run(stateWith(doc(grid()), (st) => cellText(st, 0, 0)), deleteTable)
    expect(s.doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual(['paragraph'])
  })

  it('does nothing outside a table', () => {
    expect(deleteTable(stateWith(doc(p('Intro'))))).toBe(false)
  })
})


/** Header row and header column: corner, Name, Owner / VPN, Ana / Mail, Bo. */
const crossHeaders = () =>
  table(tr(th(''), th('Name'), th('Owner')), tr(th('VPN'), td('Ana'), td('x')), tr(th('Mail'), td('Bo'), td('y')))

describe('insertRowAt', () => {
  it('inserts between rows with the caret in the new row', () => {
    const s = run(stateWith(doc(grid(), p()), (st) => cellText(st, 1, 1)), insertRowAt(2))
    expect(rows(s)).toEqual([['#Name', '#Owner'], ['VPN', 'Ana'], ['', ''], ['Mail', 'Bo']])
    expect(s.selection.from).toBe(cellText(s, 2, 1))
  })

  it('makes a row inserted above the header the header, and the old header a body row', () => {
    const s = run(stateWith(doc(grid(), p()), (st) => cellText(st, 0, 0)), insertRowAt(0))
    expect(rows(s)).toEqual([['#', '#'], ['Name', 'Owner'], ['VPN', 'Ana'], ['Mail', 'Bo']])
    expect(s.selection.from).toBe(cellText(s, 0, 0))
  })

  it('keeps a header column when promoting the header row', () => {
    const s = run(stateWith(doc(crossHeaders(), p()), (st) => cellText(st, 1, 1)), insertRowAt(0))
    expect(rows(s)).toEqual([['#', '#', '#'], ['#', 'Name', 'Owner'], ['#VPN', 'Ana', 'x'], ['#Mail', 'Bo', 'y']])
  })

  it('is one transaction', () => {
    const start = stateWith(doc(grid(), p()), (st) => cellText(st, 0, 0))
    let count = 0
    insertRowAt(0)(start, () => count++)
    expect(count).toBe(1)
  })

  it('adds a plain row above a body row', () => {
    const plain = table(tr(td('a'), td('b')), tr(td('c'), td('d')))
    const s = run(stateWith(doc(plain, p()), (st) => cellText(st, 0, 0)), insertRowAt(0))
    expect(rows(s)).toEqual([['', ''], ['a', 'b'], ['c', 'd']])
  })

  it('refuses an index outside the table, or a caret outside one', () => {
    expect(insertRowAt(9)(stateWith(doc(grid(), p()), (st) => cellText(st, 0, 0)))).toBe(false)
    expect(insertRowAt(0)(stateWith(doc(p('x'), grid())))).toBe(false)
  })
})

describe('insertColumnAt', () => {
  it('inserts between columns with the caret in the new column', () => {
    const s = run(stateWith(doc(grid(), p()), (st) => cellText(st, 1, 0)), insertColumnAt(1))
    expect(rows(s)).toEqual([['#Name', '#', '#Owner'], ['VPN', '', 'Ana'], ['Mail', '', 'Bo']])
    expect(s.selection.from).toBe(cellText(s, 1, 1))
  })

  it('makes a column inserted left of a header column the header column', () => {
    const s = run(stateWith(doc(crossHeaders(), p()), (st) => cellText(st, 1, 0)), insertColumnAt(0))
    expect(rows(s)).toEqual([['#', '#', '#Name', '#Owner'], ['#', 'VPN', 'Ana', 'x'], ['#', 'Mail', 'Bo', 'y']])
  })

  it('adds a plain column left of a body column', () => {
    const s = run(stateWith(doc(grid(), p()), (st) => cellText(st, 1, 0)), insertColumnAt(0))
    expect(rows(s)).toEqual([['#', '#Name', '#Owner'], ['', 'VPN', 'Ana'], ['', 'Mail', 'Bo']])
  })
})

describe('selecting rows, columns and tables', () => {
  it('selects a row, a column, a range and every cell', () => {
    const start = stateWith(doc(p('x'), grid()))
    const tablePos = 3
    const row = run(start, selectRow(tablePos, 1)).selection as CellSelection
    expect(row.isRowSelection()).toBe(true)
    expect(row.$anchorCell.pos).toBe(cellPos(start, 1, 0))
    const col = run(start, selectColumn(tablePos, 1)).selection as CellSelection
    expect(col.isColSelection()).toBe(true)
    const range = run(start, selectRow(tablePos, 1, 2)).selection as CellSelection
    expect(range.$headCell.pos).toBe(cellPos(start, 2, 1))
    const all = run(start, selectAllCells(tablePos)).selection as CellSelection
    expect(all.isRowSelection() && all.isColSelection()).toBe(true)
  })

  it('Mod-a selects the cell text, then every cell, then gives way', () => {
    const start = stateWith(doc(grid(), p()), (st) => cellText(st, 1, 0) + 1)
    const text = run(start, selectAllInTable)
    expect(text.selection.from).toBe(cellText(start, 1, 0))
    expect(text.selection.to).toBe(cellText(start, 1, 0) + 3)
    const all = run(text, selectAllInTable).selection as CellSelection
    expect(all.isRowSelection() && all.isColSelection()).toBe(true)
    expect(selectAllInTable(run(text, selectAllInTable))).toBe(false)
  })

  it('deletes the table when every cell is selected, not for fewer', () => {
    const start = stateWith(doc(p('x'), grid(), p('y')))
    expect(deleteSelectedCellsTable(run(start, selectRow(3, 1)))).toBe(false)
    const s = run(run(start, selectAllCells(3)), deleteSelectedCellsTable)
    expect(s.doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual(['paragraph', 'paragraph'])
  })
})

describe('alignSelection', () => {
  it('aligns the caret column, or just the selected cells', () => {
    const column = run(stateWith(doc(grid(), p()), (st) => cellText(st, 1, 1)), alignSelection('right'))
    expect(column.doc.child(0).child(2).child(1).attrs.align).toBe('right')
    expect(column.doc.child(0).child(0).child(1).attrs.align).toBe('right')
    const cells = run(stateWith(doc(grid(), p()), (st) => [cellPos(st, 1, 0), cellPos(st, 1, 1)]), alignSelection('center'))
    expect(cells.doc.child(0).child(1).child(0).attrs.align).toBe('center')
    expect(cells.doc.child(0).child(2).child(0).attrs.align).toBeNull()
  })

  it('stores left as no alignment', () => {
    const right = run(stateWith(doc(grid(), p()), (st) => cellText(st, 1, 1)), alignSelection('right'))
    const left = run(right, alignSelection('left'))
    expect(left.doc.child(0).child(1).child(1).attrs.align).toBeNull()
  })
})

describe('columnHasWidth', () => {
  it('reads widths per column, through a colspan', () => {
    const sized = table(
      tr(schema.nodes.table_cell.create({ colspan: 2, colwidth: [0, 120] }, p('wide')), td('c')),
      tr(td('a'), td('b'), td('c')),
    )
    expect(columnHasWidth(sized, 0)).toBe(false)
    expect(columnHasWidth(sized, 1)).toBe(true)
    expect(columnHasWidth(sized, 2)).toBe(false)
  })
})

describe('tableToMarkdown', () => {
  const md = (node: Node) => tableToMarkdown(node)

  it('writes a GFM table with alignment, marks and escaped pipes', () => {
    const strong = schema.marks.strong.create()
    const code = schema.marks.code.create()
    const body = tr(
      schema.nodes.table_cell.create(null, schema.nodes.paragraph.create(null, [schema.text('10.0.0.1', [code])])),
      schema.nodes.table_cell.create({ align: 'right' }, schema.nodes.paragraph.create(null, [schema.text('a|b ', null), schema.text('core', [strong])])),
      schema.nodes.table_cell.create({ align: 'center' }, p('')),
    )
    const out = md(table(tr(th('Address'), th('Notes'), th('Up')), body))
    expect(out.markdown).toBe(true)
    expect(out.text).toBe(['| Address | Notes | Up |', '| --- | ---: | :---: |', '| `10.0.0.1` | a\\|b **core** |  |'].join('\n'))
  })

  it('escapes backslashes in text and pipes everywhere, leaving code backslashes literal', () => {
    const code = schema.marks.code.create()
    const body = tr(
      schema.nodes.table_cell.create(null, schema.nodes.paragraph.create(null, [schema.text('C:\\temp\\|x', null)])),
      schema.nodes.table_cell.create(null, schema.nodes.paragraph.create(null, [schema.text('grep a\\|b', [code])])),
    )
    const out = md(table(tr(th('Path'), th('Command')), body))
    expect(out.markdown).toBe(true)
    expect(out.text.split('\n')[2]).toBe('| C:\\\\temp\\\\\\|x | `grep a\\\\|b` |')
  })

  it('falls back to HTML for a header column', () => {
    const out = md(crossHeaders())
    expect(out.markdown).toBe(false)
    expect(out.text).toContain('<th>VPN</th>')
    expect(out.text.startsWith('<table>')).toBe(true)
  })

  it('falls back to HTML for merged cells, keeping the span and alignment', () => {
    const merged = table(
      tr(th('A'), th('B')),
      tr(schema.nodes.table_cell.create({ colspan: 2, align: 'center' }, p('a < b & c'))),
    )
    const out = md(merged)
    expect(out.markdown).toBe(false)
    expect(out.text).toContain('<td colspan="2" align="center">a &lt; b &amp; c</td>')
  })

  it('falls back to HTML for a table with no header row, or a list in a cell', () => {
    expect(md(table(tr(td('a'), td('b')), tr(td('c'), td('d')))).markdown).toBe(false)
    const list = schema.nodes.bullet_list.create(null, schema.nodes.list_item.create(null, p('item')))
    expect(md(table(tr(th('A')), tr(schema.nodes.table_cell.create(null, list)))).markdown).toBe(false)
  })
})
