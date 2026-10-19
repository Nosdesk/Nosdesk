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
  addColumn,
  addColumnAfter,
  addColumnBefore,
  addRow,
  addRowAfter,
  addRowBefore,
  cellAround,
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
  setCellAttr,
  splitCell,
  toggleHeader,
} from 'prosemirror-tables'
import type { TableRect } from 'prosemirror-tables'

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

/** Whether every cell of row `row` in `table` is a header cell. */
export function rowIsAllHeader(table: ProseMirrorNode, row: number, map = TableMap.get(table)): boolean {
  if (row < 0 || row >= map.height) return false
  for (let col = 0; col < map.width; col++) {
    if (table.nodeAt(map.map[row * map.width + col]!)!.type.spec.tableRole !== 'header_cell') return false
  }
  return true
}

/** Whether every cell of column `col` in `table` is a header cell. */
export function columnIsAllHeader(table: ProseMirrorNode, col: number, map = TableMap.get(table)): boolean {
  if (col < 0 || col >= map.width) return false
  for (let row = 0; row < map.height; row++) {
    if (table.nodeAt(map.map[row * map.width + col]!)!.type.spec.tableRole !== 'header_cell') return false
  }
  return true
}

/** The table holding the selection, with its map, or null. */
export function tableOfSelection(state: EditorState): TableRect | null {
  return isInTable(state) ? selectedRect(state) : null
}

/**
 * Turn the first row (or column) into the header and the old header into a
 * body row (or column) after inserting a row above, or a column left of, a
 * header. A header column (or row) the table had keeps its first cell.
 */
function promoteHeader(
  tr: Transaction,
  tableStart: number,
  kind: 'row' | 'column',
  { headerRow, headerColumn }: { headerRow: boolean; headerColumn: boolean },
): void {
  const table = tr.doc.resolve(tableStart).parent
  const map = TableMap.get(table)
  const { schema } = table.type
  const seen = new Set<number>()
  const setType = (row: number, col: number, header: boolean) => {
    const relative = map.map[row * map.width + col]!
    if (seen.has(relative)) return
    seen.add(relative)
    const cell = table.nodeAt(relative)!
    const type = schema.nodes[header ? 'table_header' : 'table_cell']!
    if (cell.type !== type) tr.setNodeMarkup(tableStart + relative, type, cell.attrs)
  }
  if (kind === 'row') {
    for (let col = 0; col < map.width; col++) setType(0, col, true)
    if (map.height > 1) for (let col = 0; col < map.width; col++) setType(1, col, headerColumn && col === 0)
  } else {
    for (let row = 0; row < map.height; row++) setType(row, 0, true)
    if (map.width > 1) for (let row = 0; row < map.height; row++) setType(row, 1, headerRow && row === 0)
  }
}

/** `tr` with the caret in cell (row, col) of the table starting at `tableStart`. */
function caretInCell(tr: Transaction, tableStart: number, row: number, col: number): Transaction {
  const map = TableMap.get(tr.doc.resolve(tableStart).parent)
  const r = Math.max(0, Math.min(map.height - 1, row))
  const c = Math.max(0, Math.min(map.width - 1, col))
  return tr.setSelection(TextSelection.near(tr.doc.resolve(tableStart + map.map[r * map.width + c]! + 1)))
}

/**
 * Insert an empty row at `index` (0 is above the first row, the row count is
 * below the last) in the table holding the selection, caret in the new row.
 * Inserted above a header row, the new row becomes the header and the old
 * one a body row, so the header stays first. One transaction, one undo step.
 */
export function insertRowAt(index: number): Command {
  return (state, dispatch) => {
    const rect = tableOfSelection(state)
    if (!rect || index < 0 || index > rect.map.height) return false
    if (dispatch) {
      const { map, table, tableStart } = rect
      const promote = index === 0 && rowIsAllHeader(table, 0, map)
      const headerColumn = columnIsAllHeader(table, 0, map)
      const tr = addRow(state.tr, rect, index)
      if (promote) promoteHeader(tr, tableStart, 'row', { headerRow: true, headerColumn })
      dispatch(caretInCell(tr, tableStart, index, rect.left).scrollIntoView())
    }
    return true
  }
}

/**
 * Insert an empty column at `index` in the table holding the selection,
 * caret in the new column. Inserted left of a header column, the new column
 * becomes the header and the old one a body column.
 */
export function insertColumnAt(index: number): Command {
  return (state, dispatch) => {
    const rect = tableOfSelection(state)
    if (!rect || index < 0 || index > rect.map.width) return false
    if (dispatch) {
      const { map, table, tableStart } = rect
      const promote = index === 0 && columnIsAllHeader(table, 0, map)
      const headerRow = rowIsAllHeader(table, 0, map)
      const tr = addColumn(state.tr, rect, index)
      if (promote) promoteHeader(tr, tableStart, 'column', { headerRow, headerColumn: true })
      dispatch(caretInCell(tr, tableStart, rect.top, index).scrollIntoView())
    }
    return true
  }
}

/** A row above the selection, with header promotion on the first row. */
export const insertRowAbove: Command = (state, dispatch) => {
  const rect = tableOfSelection(state)
  return rect ? insertRowAt(rect.top)(state, dispatch) : false
}

/** A row below the selection. */
export const insertRowBelow: Command = (state, dispatch) => {
  const rect = tableOfSelection(state)
  return rect ? insertRowAt(rect.bottom)(state, dispatch) : false
}

/** A column left of the selection, with header promotion on the first column. */
export const insertColumnLeft: Command = (state, dispatch) => {
  const rect = tableOfSelection(state)
  return rect ? insertColumnAt(rect.left)(state, dispatch) : false
}

/** A column right of the selection. */
export const insertColumnRight: Command = (state, dispatch) => {
  const rect = tableOfSelection(state)
  return rect ? insertColumnAt(rect.right)(state, dispatch) : false
}

/**
 * Select the cells from (row1, col1) to (row2, col2) of the table at
 * `tablePos`. Indices outside the table are clamped.
 */
export function selectCells(tablePos: number, row1: number, col1: number, row2: number, col2: number): Command {
  return (state, dispatch) => {
    const table = state.doc.nodeAt(tablePos)
    if (!table || table.type.spec.tableRole !== 'table') return false
    if (dispatch) {
      const map = TableMap.get(table)
      const clampRow = (r: number) => Math.max(0, Math.min(map.height - 1, r))
      const clampCol = (c: number) => Math.max(0, Math.min(map.width - 1, c))
      const at = (r: number, c: number) => tablePos + 1 + map.map[clampRow(r) * map.width + clampCol(c)]!
      dispatch(state.tr.setSelection(CellSelection.create(state.doc, at(row1, col1), at(row2, col2))))
    }
    return true
  }
}

/** Select every cell of row `row` of the table at `tablePos`. */
export function selectRow(tablePos: number, row: number, toRow = row): Command {
  return (state, dispatch) => {
    const table = state.doc.nodeAt(tablePos)
    if (!table) return false
    return selectCells(tablePos, row, 0, toRow, TableMap.get(table).width - 1)(state, dispatch)
  }
}

/** Select every cell of column `col` of the table at `tablePos`. */
export function selectColumn(tablePos: number, col: number, toCol = col): Command {
  return (state, dispatch) => {
    const table = state.doc.nodeAt(tablePos)
    if (!table) return false
    return selectCells(tablePos, 0, col, TableMap.get(table).height - 1, toCol)(state, dispatch)
  }
}

/** Select every cell of the table at `tablePos`. */
export function selectAllCells(tablePos: number): Command {
  return (state, dispatch) => {
    const table = state.doc.nodeAt(tablePos)
    if (!table) return false
    const map = TableMap.get(table)
    return selectCells(tablePos, 0, 0, map.height - 1, map.width - 1)(state, dispatch)
  }
}

/** Whether the selection is a cell selection covering the whole table. */
export function isWholeTableSelected(state: EditorState): boolean {
  const { selection } = state
  return selection instanceof CellSelection && selection.isRowSelection() && selection.isColSelection()
}

/** Backspace or Delete with every cell selected: delete the table. */
export const deleteSelectedCellsTable: Command = (state, dispatch) =>
  isWholeTableSelected(state) ? deleteTable(state, dispatch) : false

/**
 * Mod-a in a cell: the cell's text, then every cell of the table, then
 * (outside this command) the document.
 */
export const selectAllInTable: Command = (state, dispatch) => {
  const { selection } = state
  if (selection instanceof CellSelection) {
    if (isWholeTableSelected(state)) return false
    return selectAllCells(selection.$headCell.start(-1) - 1)(state, dispatch)
  }
  const $cell = cellAround(selection.$head)
  if (!$cell || cellAround(selection.$anchor)?.pos !== $cell.pos) return false
  const text = TextSelection.between(
    state.doc.resolve($cell.pos + 1),
    state.doc.resolve($cell.pos + $cell.nodeAfter!.nodeSize - 1),
  )
  if (selection.from <= text.from && selection.to >= text.to) {
    return selectAllCells($cell.start(-1) - 1)(state, dispatch)
  }
  dispatch?.(state.tr.setSelection(text))
  return true
}

/** The alignment of column `col` (its first body cell), `left` when unset. */
export function columnAlignment(table: ProseMirrorNode, col: number, map = TableMap.get(table)): CellAlign {
  const row = map.height > 1 ? 1 : 0
  const align = table.nodeAt(map.map[row * map.width + col]!)!.attrs.align as CellAlign | null
  return align ?? 'left'
}

/** The alignment of the selected cells (the first one), `left` when unset. */
export function selectionAlignment(state: EditorState): CellAlign {
  if (!isInTable(state)) return 'left'
  const rect = selectedRect(state)
  const align = rect.table.nodeAt(rect.map.map[rect.top * rect.map.width + rect.left]!)!.attrs.align
  return (align as CellAlign | null) ?? 'left'
}

/**
 * Align the selected cells, or the caret's column when no cells are
 * selected. Left is the default, so it is stored as no alignment.
 */
export function alignSelection(align: CellAlign): Command {
  return (state, dispatch) => {
    if (!isInTable(state)) return false
    const value = align === 'left' ? null : align
    if (state.selection instanceof CellSelection) return setCellAttr('align', value)(state, dispatch)
    return alignColumn(value)(state, dispatch)
  }
}

/** Whether column `col` of `table` has a set width. */
export function columnHasWidth(table: ProseMirrorNode, col: number, map = TableMap.get(table)): boolean {
  for (let row = 0; row < map.height; row++) {
    const relative = map.map[row * map.width + col]!
    const widths = table.nodeAt(relative)!.attrs.colwidth as number[] | null
    const index = col - map.colCount(relative)
    if ((widths?.[index] ?? 0) > 0) return true
  }
  return false
}

/** Whether the cell at the selection's anchor spans more than one row or column. */
export function anchorCellIsMerged(state: EditorState): boolean {
  const { selection } = state
  const cell = selection instanceof CellSelection ? selection.$anchorCell.nodeAfter : null
  return !!cell && ((cell.attrs.colspan as number) > 1 || (cell.attrs.rowspan as number) > 1)
}

/** Whether the cell selection holds more than one cell (a merged cell counts once). */
export function selectionIsMultiCell(state: EditorState): boolean {
  const { selection } = state
  if (!(selection instanceof CellSelection)) return false
  let count = 0
  selection.forEachCell(() => count++)
  return count > 1
}

const MARK_WRAP: Record<string, [string, string]> = {
  strong: ['**', '**'],
  em: ['*', '*'],
  code: ['`', '`'],
}

/** A cell's inline content as one line of Markdown. */
function cellMarkdown(cell: ProseMirrorNode): string {
  const parts: string[] = []
  cell.forEach((block, _offset, index) => {
    if (index > 0) parts.push('<br>')
    block.descendants((node) => {
      if (node.isText) {
        const code = node.marks.some((m) => m.type.name === 'code')
        let text = code ? node.text! : node.text!.replace(/([\\*_`[\]])/g, '\\$1')
        for (const mark of node.marks) {
          const wrap = MARK_WRAP[mark.type.name]
          if (wrap) text = wrap[0] + text + wrap[1]
        }
        const link = node.marks.find((m) => m.type.name === 'link')
        if (link) text = `[${text}](${link.attrs.href as string})`
        parts.push(text)
      } else if (node.type.name === 'hard_break') {
        parts.push('<br>')
      } else if (node.type.name === 'mention') {
        parts.push(`@${node.attrs.name as string}`)
      } else if (node.isLeaf) {
        parts.push(node.textContent)
      }
      return true
    })
  })
  // GFM splits a row into cells on unescaped pipes before it parses inlines,
  // so every pipe is written `\|`, inside code spans too. Backslashes are
  // already escaped outside code above; inside code they stay literal, and
  // GFM still reads `\|` there as a pipe.
  return parts.join('').split('|').join('\\|').replace(/\n/g, '<br>')
}

function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/**
 * `table` as text to paste elsewhere: a GFM pipe table when Markdown can
 * hold it (a header row and nothing else as headers, no merged cells, one
 * paragraph per cell), otherwise an HTML table, which Markdown renderers
 * accept as is.
 */
export function tableToMarkdown(table: ProseMirrorNode): { text: string; markdown: boolean } {
  const map = TableMap.get(table)
  let exact = true
  for (let row = 0; row < map.height && exact; row++) {
    for (let col = 0; col < map.width; col++) {
      const cell = table.nodeAt(map.map[row * map.width + col]!)!
      const header = cell.type.spec.tableRole === 'header_cell'
      if ((cell.attrs.colspan as number) > 1 || (cell.attrs.rowspan as number) > 1) exact = false
      else if (header !== (row === 0)) exact = false
      else if (cell.childCount !== 1 || cell.firstChild!.type.name !== 'paragraph') exact = false
      if (!exact) break
    }
  }
  if (exact) {
    const line = (cells: string[]) => `| ${cells.join(' | ')} |`
    const rows: string[] = []
    for (let row = 0; row < map.height; row++) {
      const cells: string[] = []
      for (let col = 0; col < map.width; col++) cells.push(cellMarkdown(table.nodeAt(map.map[row * map.width + col]!)!))
      rows.push(line(cells))
    }
    const aligns: string[] = []
    for (let col = 0; col < map.width; col++) {
      const align = columnAlignment(table, col, map)
      aligns.push(align === 'center' ? ':---:' : align === 'right' ? '---:' : '---')
    }
    return { text: [rows[0]!, line(aligns), ...rows.slice(1)].join('\n'), markdown: true }
  }
  let html = '<table>\n'
  table.forEach((row) => {
    html += '  <tr>'
    row.forEach((cell) => {
      const tag = cell.type.spec.tableRole === 'header_cell' ? 'th' : 'td'
      const colspan = cell.attrs.colspan as number
      const rowspan = cell.attrs.rowspan as number
      const align = cell.attrs.align as string | null
      const attrs =
        (colspan > 1 ? ` colspan="${colspan}"` : '') +
        (rowspan > 1 ? ` rowspan="${rowspan}"` : '') +
        (align === 'left' || align === 'center' || align === 'right' ? ` align="${align}"` : '')
      html += `<${tag}${attrs}>${escapeHtml(cell.textBetween(0, cell.content.size, ' ', ' '))}</${tag}>`
    })
    html += '</tr>\n'
  })
  return { text: `${html}</table>`, markdown: false }
}
