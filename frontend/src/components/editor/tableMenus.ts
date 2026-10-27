/**
 * What the table controls show: which table, row and column the selection is
 * in, and the row, column, table, selection and grouped (toolbar) menus.
 *
 * Everything here reads an `EditorState` and returns plain data, so the
 * menus are the same wherever they open (a handle's popover, the phone's
 * sheet, the toolbar) and are tested without a view. Each item's `run`
 * selects the row or column it belongs to first, then runs the command on
 * that selection, so a menu acts on the row it was opened for even when the
 * caret is elsewhere in the table.
 */
import type { Command, EditorState } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import { CellSelection, TableMap, cellAround, moveTableColumn, moveTableRow, selectedRect } from 'prosemirror-tables'
import type { MenuItem } from '@/components/common/MenuList.vue'
import { ICON_REGISTRY } from '@/components/common/icons'
import {
  alignSelection,
  anchorCellIsMerged,
  clearCells,
  columnAlignment,
  columnHasWidth,
  columnIsAllHeader,
  deleteColumn,
  deleteRow,
  insertColumnAt,
  insertRowAt,
  isWholeTableSelected,
  mergeCells,
  resetColumnWidth,
  resetColumnWidths,
  rowIsAllHeader,
  selectColumn,
  selectRow,
  selectionAlignment,
  selectionIsMultiCell,
  splitCell,
  tableHasWidths,
  tableOfSelection,
  toggleHeaderColumn,
  toggleHeaderRow,
} from './tableCommands'
import type { CellAlign } from './tableCommands'

/** Where the selection is, for the handles and the toolbar's Table button. */
export interface TableUiState {
  /** Position of the table node holding the selection. */
  tablePos: number
  /** Row and column of the caret's (or the selection head's) cell. */
  row: number
  col: number
  /** Whole rows and whole columns the cell selection covers. */
  rows: number[]
  cols: number[]
  /** Every cell is selected. */
  all: boolean
  /** A cell selection over more than one cell, or a merged cell. */
  cells: boolean
}

/** Where the selection is in a table, or null outside one. */
export function tableUiState(state: EditorState): TableUiState | null {
  const { selection } = state
  const $cell = selection instanceof CellSelection ? selection.$headCell : cellAround(selection.$head)
  if (!$cell) return null
  const table = $cell.node(-1)
  const tableStart = $cell.start(-1)
  const map = TableMap.get(table)
  const rect = map.findCell($cell.pos - tableStart)
  const rows: number[] = []
  const cols: number[] = []
  let all = false
  let cells = false
  if (selection instanceof CellSelection) {
    const selected = selectedRect(state)
    const rowSel = selection.isRowSelection()
    const colSel = selection.isColSelection()
    if (rowSel) for (let r = selected.top; r < selected.bottom; r++) rows.push(r)
    if (colSel) for (let c = selected.left; c < selected.right; c++) cols.push(c)
    all = rowSel && colSel
    cells = selectionIsMultiCell(state) || anchorCellIsMerged(state)
  }
  return { tablePos: tableStart - 1, row: rect.top, col: rect.left, rows, cols, all, cells }
}

/** A menu row plus what choosing it does. */
export interface TableMenuItem extends MenuItem {
  run?: (view: EditorView) => void
}

export interface TableMenuContext {
  t: (key: string, args?: Record<string, string | number>) => string
  /** The modifier key's label, `⌘` or `Ctrl`. */
  modKey: string
  /** Delete the table holding the selection (and offer an undo). */
  deleteTable: (view: EditorView) => void
  /** Copy the table holding the selection as Markdown (and say so). */
  copyMarkdown: (view: EditorView) => void
}

const icon = (name: keyof typeof ICON_REGISTRY) => ICON_REGISTRY[name].d

/** `view`'s table position, or null when the selection has left the table. */
function tablePosIn(state: EditorState): number | null {
  const rect = tableOfSelection(state)
  return rect ? rect.tableStart - 1 : null
}

/** Select `select`'s cells in the current table, then run each command. */
function after(select: (tablePos: number) => Command, ...commands: Command[]) {
  return (view: EditorView) => {
    const tablePos = tablePosIn(view.state)
    if (tablePos === null) return
    select(tablePos)(view.state, view.dispatch, view)
    for (const command of commands) command(view.state, view.dispatch, view)
  }
}

function onRow(row: number, ...commands: Command[]) {
  return after((tablePos) => selectRow(tablePos, row), ...commands)
}

function onColumn(col: number, ...commands: Command[]) {
  return after((tablePos) => selectColumn(tablePos, col), ...commands)
}

/** The row menu for row `row` of the table holding the selection. */
export function rowMenu(state: EditorState, row: number, { t, modKey }: TableMenuContext): TableMenuItem[] {
  const rect = tableOfSelection(state)
  if (!rect) return []
  const last = rect.map.height - 1
  const header = rowIsAllHeader(rect.table, 0, rect.map)
  return [
    { id: 'row-above', label: t('editor-table-insert-row-above'), icon: icon('rowAbove'), run: onRow(row, insertRowAt(row)) },
    {
      id: 'row-below',
      label: t('editor-table-insert-row-below'),
      icon: icon('rowBelow'),
      trailing: `${modKey}↩`,
      run: onRow(row, insertRowAt(row + 1)),
    },
    {
      id: 'row-up',
      label: t('editor-table-move-up'),
      icon: icon('arrowUp'),
      divider: true,
      disabled: row === 0 || (row === 1 && header),
      run: onRow(row, moveTableRow({ from: row, to: row - 1 })),
    },
    {
      id: 'row-down',
      label: t('editor-table-move-down'),
      icon: icon('arrowDown'),
      disabled: row === last || (row === 0 && header),
      run: onRow(row, moveTableRow({ from: row, to: row + 1 })),
    },
    ...(row === 0
      ? [
          {
            id: 'row-header',
            label: t('editor-table-header-row'),
            icon: icon('headerRow'),
            checked: header,
            divider: true,
            run: onRow(row, toggleHeaderRow),
          },
        ]
      : []),
    { id: 'row-clear', label: t('editor-table-clear'), icon: icon('eraser'), divider: row !== 0, run: onRow(row, clearCells) },
    {
      id: 'row-delete',
      label: t('editor-table-delete-row'),
      icon: icon('trash'),
      danger: true,
      divider: true,
      run: onRow(row, deleteRow),
    },
  ]
}

function alignItem(id: string, align: CellAlign, current: CellAlign, t: TableMenuContext['t'], run: TableMenuItem['run']): TableMenuItem {
  const icons = { left: 'alignLeft', center: 'alignCenter', right: 'alignRight' } as const
  return { id, label: t(`editor-table-align-${align}`), icon: icon(icons[align]), checked: current === align, run }
}

/** The column menu for column `col` of the table holding the selection. */
export function columnMenu(state: EditorState, col: number, { t }: TableMenuContext): TableMenuItem[] {
  const rect = tableOfSelection(state)
  if (!rect) return []
  const last = rect.map.width - 1
  const header = columnIsAllHeader(rect.table, 0, rect.map)
  const align = columnAlignment(rect.table, col, rect.map)
  const sized = columnHasWidth(rect.table, col, rect.map)
  return [
    { id: 'col-left', label: t('editor-table-insert-column-left'), icon: icon('columnLeft'), run: onColumn(col, insertColumnAt(col)) },
    {
      id: 'col-right',
      label: t('editor-table-insert-column-right'),
      icon: icon('columnRight'),
      run: onColumn(col, insertColumnAt(col + 1)),
    },
    {
      id: 'col-move-left',
      label: t('editor-table-move-left'),
      icon: icon('arrowLeft'),
      divider: true,
      disabled: col === 0 || (col === 1 && header),
      run: onColumn(col, moveTableColumn({ from: col, to: col - 1 })),
    },
    {
      id: 'col-move-right',
      label: t('editor-table-move-right'),
      icon: icon('arrowRight'),
      disabled: col === last || (col === 0 && header),
      run: onColumn(col, moveTableColumn({ from: col, to: col + 1 })),
    },
    { ...alignItem('col-align-left', 'left', align, t, onColumn(col, alignSelection('left'))), divider: true },
    alignItem('col-align-center', 'center', align, t, onColumn(col, alignSelection('center'))),
    alignItem('col-align-right', 'right', align, t, onColumn(col, alignSelection('right'))),
    ...(col === 0
      ? [
          {
            id: 'col-header',
            label: t('editor-table-header-column'),
            icon: icon('headerColumn'),
            checked: header,
            divider: true,
            run: onColumn(col, toggleHeaderColumn),
          },
        ]
      : []),
    ...(sized
      ? [
          {
            id: 'col-reset',
            label: t('editor-table-reset-width'),
            icon: icon('reset'),
            divider: col !== 0,
            run: onColumn(col, resetColumnWidth),
          },
        ]
      : []),
    {
      id: 'col-clear',
      label: t('editor-table-clear'),
      icon: icon('eraser'),
      divider: col !== 0 && !sized,
      run: onColumn(col, clearCells),
    },
    {
      id: 'col-delete',
      label: t('editor-table-delete-column'),
      icon: icon('trash'),
      danger: true,
      divider: true,
      run: onColumn(col, deleteColumn),
    },
  ]
}

/** The table menu (the corner handle) for the table holding the selection. */
export function tableMenu(state: EditorState, { t, deleteTable, copyMarkdown }: TableMenuContext): TableMenuItem[] {
  const rect = tableOfSelection(state)
  if (!rect) return []
  return [
    {
      id: 'table-header-row',
      label: t('editor-table-header-row'),
      icon: icon('headerRow'),
      checked: rowIsAllHeader(rect.table, 0, rect.map),
      run: (view) => toggleHeaderRow(view.state, view.dispatch),
    },
    {
      id: 'table-header-column',
      label: t('editor-table-header-column'),
      icon: icon('headerColumn'),
      checked: columnIsAllHeader(rect.table, 0, rect.map),
      run: (view) => toggleHeaderColumn(view.state, view.dispatch),
    },
    {
      id: 'table-reset-widths',
      label: t('editor-table-reset-widths'),
      icon: icon('reset'),
      divider: true,
      disabled: !tableHasWidths(state),
      run: (view) => resetColumnWidths(view.state, view.dispatch),
    },
    { id: 'table-copy-markdown', label: t('editor-table-copy-markdown'), icon: icon('copyMd'), run: copyMarkdown },
    {
      id: 'table-delete',
      label: t('editor-table-delete-table'),
      icon: icon('trash'),
      danger: true,
      divider: true,
      trailing: isWholeTableSelected(state) ? '⌫' : undefined,
      run: deleteTable,
    },
  ]
}

/**
 * Merge, split, align and clear for a cell selection over more than one
 * cell or on a merged cell; empty otherwise.
 */
export function selectionMenu(state: EditorState, { t }: TableMenuContext): TableMenuItem[] {
  if (!(state.selection instanceof CellSelection)) return []
  const multi = selectionIsMultiCell(state)
  const merged = anchorCellIsMerged(state)
  if (!multi && !merged) return []
  const align = selectionAlignment(state)
  const command = (c: Command) => (view: EditorView) => c(view.state, view.dispatch, view)
  return [
    ...(multi ? [{ id: 'cells-merge', label: t('editor-table-merge'), icon: icon('merge'), run: command(mergeCells) }] : []),
    ...(merged ? [{ id: 'cells-split', label: t('editor-table-split'), icon: icon('split'), run: command(splitCell) }] : []),
    alignItem('cells-align-left', 'left', align, t, command(alignSelection('left'))),
    alignItem('cells-align-center', 'center', align, t, command(alignSelection('center'))),
    alignItem('cells-align-right', 'right', align, t, command(alignSelection('right'))),
    { id: 'cells-clear', label: t('editor-table-clear'), icon: icon('eraser'), trailing: '⌫', run: command(clearCells) },
  ]
}

/**
 * Every table action in one list, grouped under headings: the selected
 * cells (when there are any), the caret's row, its column, and the table.
 * The toolbar's Table button opens this, so a keyboard or phone user can
 * reach everything the handles offer.
 */
export function groupedMenu(state: EditorState, context: TableMenuContext): TableMenuItem[] {
  const ui = tableUiState(state)
  if (!ui) return []
  const { t } = context
  const strip = (items: TableMenuItem[]) => items.map((item) => ({ ...item, divider: false }))
  const cells = selectionMenu(state, context)
  return [
    ...(cells.length ? [{ id: 'heading-cells', heading: true, label: t('editor-table-menu-cells') }, ...strip(cells)] : []),
    {
      id: 'heading-row',
      heading: true,
      label: t('editor-table-menu-row', { row: ui.row + 1 }),
      divider: cells.length > 0,
    },
    ...strip(rowMenu(state, ui.row, context)),
    { id: 'heading-column', heading: true, label: t('editor-table-menu-column', { column: ui.col + 1 }), divider: true },
    ...strip(columnMenu(state, ui.col, context)),
    { id: 'heading-table', heading: true, label: t('editor-table-menu-table'), divider: true },
    ...strip(tableMenu(state, context)),
  ]
}
