/**
 * Table commands for the documentation editor: what the keyboard uses now and
 * what the table controls (row, column, table and selection menus) call.
 *
 * Every command is a plain ProseMirror `Command`: called without `dispatch` it
 * only reports whether it applies, which is how a menu greys an item out.
 * None of them checks whether this document may hold tables; the table guard
 * drops a transaction that adds one where it may not, and the insert entry is
 * hidden there.
 */
import { NodeSelection, Selection, TextSelection } from 'prosemirror-state'
import type { Command, EditorState, Transaction } from 'prosemirror-state'
import type { Node as ProseMirrorNode, NodeType } from 'prosemirror-model'
import {
  CellSelection,
  TableMap,
  addColumnAfter,
  addColumnBefore,
  addRowAfter,
  addRowBefore,
  deleteCellSelection,
  deleteColumn,
  deleteRow,
  goToNextCell,
  isInTable,
  mergeCells,
  moveTableColumn,
  moveTableRow,
  selectedRect,
  selectionCell,
  splitCell,
  toggleHeader,
} from 'prosemirror-tables'

export {
  addColumnAfter,
  addColumnBefore,
  addRowAfter,
  addRowBefore,
  deleteColumn,
  deleteRow,
  goToNextCell,
  mergeCells,
  splitCell,
}

export type CellAlign = 'left' | 'center' | 'right'

/** Columns and body rows of a table made by `insertTable`. */
export const NEW_TABLE_COLUMNS = 3
export const NEW_TABLE_BODY_ROWS = 2

function cellType(state: EditorState, header: boolean): NodeType {
  return state.schema.nodes[header ? 'table_header' : 'table_cell']!
}

/** A row of `columns` empty cells. */
export function emptyRow(state: EditorState, columns: number, header = false): ProseMirrorNode {
  const type = cellType(state, header)
  const cells = Array.from({ length: columns }, () => type.createAndFill()!)
  return state.schema.nodes.table_row!.create(null, cells)
}

/**
 * Place `table` after the top-level block holding the selection, keep a
 * paragraph after it when it would end the document, and put the caret in
 * the cell at `caret` (row, column). The table takes that block's place when
 * `replaceBlock` is set or the block is an empty paragraph.
 */
export function placeTable(
  state: EditorState,
  table: ProseMirrorNode,
  { caret = [0, 0], replaceBlock = false }: { caret?: [number, number]; replaceBlock?: boolean } = {},
): Transaction {
  const { selection } = state
  const { $from } = selection
  let from: number
  let to: number
  if ($from.depth === 0) {
    // A gap cursor or a selected top-level node.
    from = to = selection instanceof NodeSelection ? selection.to : $from.pos
  } else {
    const top = $from.node(1)
    const emptyParagraph = top.type === state.schema.nodes.paragraph && top.content.size === 0
    from = replaceBlock || emptyParagraph ? $from.before(1) : $from.after(1)
    to = $from.after(1)
  }
  const tr = state.tr.replaceWith(from, to, table)
  const tableEnd = from + table.nodeSize
  if (tableEnd === tr.doc.content.size) {
    tr.insert(tableEnd, state.schema.nodes.paragraph!.create())
  }
  const map = TableMap.get(table)
  const [row, col] = caret
  const cellPos = from + 1 + map.map[row * map.width + col]!
  tr.setSelection(TextSelection.near(tr.doc.resolve(cellPos + 1)))
  return tr.scrollIntoView()
}

/**
 * Insert a table of three columns, a header row and two rows, after the
 * current top-level block, with the caret in the first header cell.
 */
export const insertTable: Command = (state, dispatch) => {
  if (dispatch) {
    const rows = [
      emptyRow(state, NEW_TABLE_COLUMNS, true),
      ...Array.from({ length: NEW_TABLE_BODY_ROWS }, () => emptyRow(state, NEW_TABLE_COLUMNS)),
    ]
    dispatch(placeTable(state, state.schema.nodes.table!.create(null, rows)))
  }
  return true
}

/**
 * `tr`, which added rows to the table whose content starts at `tableStart`,
 * with the caret in the first cell of row `rowIndex`.
 */
function caretInRowStart(tr: Transaction, tableStart: number, rowIndex: number): Transaction {
  const start = tr.mapping.map(tableStart)
  const map = TableMap.get(tr.doc.resolve(start).parent)
  const cellPos = start + map.map[rowIndex * map.width]!
  return tr.setSelection(TextSelection.near(tr.doc.resolve(cellPos + 1))).scrollIntoView()
}

/** Tab: the next cell, or a new row when the caret is in the last cell. */
export const nextCellOrNewRow: Command = (state, dispatch) => {
  if (!isInTable(state)) return false
  if (goToNextCell(1)(state, dispatch)) return true
  return addRowBelowAndEnter(state, dispatch)
}

/** Mod-Enter in a cell: a row below the selection, caret in its first cell. */
export const addRowBelowAndEnter: Command = (state, dispatch) => {
  if (!isInTable(state)) return false
  if (!dispatch) return true
  const { bottom, tableStart } = selectedRect(state)
  addRowAfter(state, (tr) => dispatch(caretInRowStart(tr, tableStart, bottom)))
  return true
}

function moveRow(dir: -1 | 1): Command {
  return (state, dispatch) => {
    if (!isInTable(state)) return false
    const rect = selectedRect(state)
    const from = dir < 0 ? rect.top : rect.bottom - 1
    const to = from + dir
    if (to < 0 || to >= rect.map.height) return false
    return moveTableRow({ from, to })(state, dispatch)
  }
}

function moveColumn(dir: -1 | 1): Command {
  return (state, dispatch) => {
    if (!isInTable(state)) return false
    const rect = selectedRect(state)
    const from = dir < 0 ? rect.left : rect.right - 1
    const to = from + dir
    if (to < 0 || to >= rect.map.width) return false
    return moveTableColumn({ from, to })(state, dispatch)
  }
}

export const moveRowUp = moveRow(-1)
export const moveRowDown = moveRow(1)
export const moveColumnLeft = moveColumn(-1)
export const moveColumnRight = moveColumn(1)

/** Toggle header cells on the table's first row. */
export const toggleHeaderRow: Command = toggleHeader('row')
/** Toggle header cells on the table's first column. */
export const toggleHeaderColumn: Command = toggleHeader('column')

/** Every cell in the columns `left` to `right` (exclusive), once each. */
function columnCells(state: EditorState, left: number, right: number): number[] {
  const rect = selectedRect(state)
  const seen = new Set<number>()
  for (let row = 0; row < rect.map.height; row++) {
    for (let col = left; col < right; col++) {
      seen.add(rect.tableStart + rect.map.map[row * rect.map.width + col]!)
    }
  }
  return [...seen]
}

/** Align the selected columns (whole columns), or clear their alignment. */
export function alignColumn(align: CellAlign | null): Command {
  return (state, dispatch) => {
    if (!isInTable(state)) return false
    if (dispatch) {
      const rect = selectedRect(state)
      const tr = state.tr
      for (const pos of columnCells(state, rect.left, rect.right)) {
        const cell = tr.doc.nodeAt(pos)!
        if (cell.attrs.align !== align) tr.setNodeMarkup(pos, null, { ...cell.attrs, align })
      }
      if (tr.docChanged) dispatch(tr)
    }
    return true
  }
}

/** Empty the selected cells, or the cell holding the caret. */
export const clearCells: Command = (state, dispatch) => {
  if (state.selection instanceof CellSelection) return deleteCellSelection(state, dispatch)
  if (!isInTable(state)) return false
  if (dispatch) {
    const $cell = selectionCell(state)
    const cell = $cell.nodeAfter!
    const empty = cellType(state, false).createAndFill()!.content
    if (!cell.content.eq(empty)) {
      dispatch(state.tr.replaceWith($cell.pos + 1, $cell.pos + cell.nodeSize - 1, empty))
    }
  }
  return true
}

/** `colwidth` with column `index` unset; null once no column has a width. */
function withoutWidth(colwidth: readonly number[] | null, index: number | null): number[] | null {
  if (!colwidth) return null
  const next = colwidth.map((w, i) => (index === null || i === index ? 0 : w))
  return next.some((w) => w > 0) ? next : null
}

/**
 * Clear set widths in the table starting at `tableStart`: columns `left` to
 * `right` (exclusive), or every column when `columns` is null.
 */
function resetWidths(state: EditorState, tableStart: number, columns: [number, number] | null): Transaction | null {
  const map = TableMap.get(state.doc.resolve(tableStart).parent)
  const [left, right] = columns ?? [0, map.width]
  const tr = state.tr
  for (let col = left; col < right; col++) {
    for (let row = 0; row < map.height; row++) {
      const index = row * map.width + col
      if (row > 0 && map.map[index] === map.map[index - map.width]) continue
      const relative = map.map[index]!
      const pos = tableStart + relative
      const cell = tr.doc.nodeAt(pos)!
      const indexInCell = columns ? col - map.colCount(relative) : null
      const current = cell.attrs.colwidth as number[] | null
      const colwidth = withoutWidth(current, indexInCell)
      if (JSON.stringify(colwidth) !== JSON.stringify(current)) {
        tr.setNodeMarkup(pos, null, { ...cell.attrs, colwidth })
      }
    }
  }
  return tr.docChanged ? tr : null
}

/** Whether any column of the table holding the selection has a set width. */
export function tableHasWidths(state: EditorState): boolean {
  if (!isInTable(state)) return false
  let found = false
  selectedRect(state).table.descendants((node) => {
    if ((node.attrs.colwidth as number[] | null)?.some((w) => w > 0)) found = true
    return !found && !node.type.spec.tableRole?.includes('cell')
  })
  return found
}

/** Clear the set width of the selected columns, so they share the space. */
export const resetColumnWidth: Command = (state, dispatch) => {
  if (!isInTable(state)) return false
  const rect = selectedRect(state)
  const tr = resetWidths(state, rect.tableStart, [rect.left, rect.right])
  if (!tr) return false
  dispatch?.(tr)
  return true
}

/** Clear every set column width in the table. */
export const resetColumnWidths: Command = (state, dispatch) => {
  if (!isInTable(state)) return false
  const tr = resetWidths(state, selectedRect(state).tableStart, null)
  if (!tr) return false
  dispatch?.(tr)
  return true
}

/**
 * Clear the width of the column whose right border is at the cell at
 * `cellPos` (a column-resize handle position), or null when it has none.
 */
export function resetColumnWidthAtHandle(state: EditorState, cellPos: number): Transaction | null {
  const $cell = state.doc.resolve(cellPos)
  const tableStart = $cell.start(-1)
  const map = TableMap.get($cell.node(-1))
  const col = map.colCount(cellPos - tableStart) + ($cell.nodeAfter!.attrs.colspan as number) - 1
  return resetWidths(state, tableStart, [col, col + 1])
}

/** The table node the selection is in or has selected, with its position. */
function tableAround(state: EditorState): { pos: number; node: ProseMirrorNode } | null {
  const { selection } = state
  if (selection instanceof NodeSelection && selection.node.type.spec.tableRole === 'table') {
    return { pos: selection.from, node: selection.node }
  }
  const $pos = selection.$anchor
  for (let d = $pos.depth; d > 0; d--) {
    if ($pos.node(d).type.spec.tableRole === 'table') return { pos: $pos.before(d), node: $pos.node(d) }
  }
  return null
}

/** Delete the table holding (or selected by) the selection. */
export const deleteTable: Command = (state, dispatch) => {
  const found = tableAround(state)
  if (!found) return false
  if (dispatch) {
    const { pos, node } = found
    const tr = state.tr
    if (state.doc.childCount === 1) {
      tr.replaceWith(pos, pos + node.nodeSize, state.schema.nodes.paragraph!.create())
    } else {
      tr.delete(pos, pos + node.nodeSize)
    }
    tr.setSelection(Selection.near(tr.doc.resolve(Math.min(pos, tr.doc.content.size)), -1))
    dispatch(tr.scrollIntoView())
  }
  return true
}

/** Backspace or Delete on a selected table node. */
export const deleteSelectedTable: Command = (state, dispatch) => {
  const { selection } = state
  if (!(selection instanceof NodeSelection) || selection.node.type.spec.tableRole !== 'table') return false
  return deleteTable(state, dispatch)
}
