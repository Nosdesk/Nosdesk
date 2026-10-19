/**
 * Table editing in the collaborative editor: cell selection, the keyboard,
 * column resizing, the `|` shortcut, and tables through the clipboard.
 *
 * Two of these exist because of y-prosemirror rather than tables:
 *
 * - prosemirror-tables repairs ragged tables (`fixTables`) in an appended
 *   transaction. Run after a remote change, that repair stays in this
 *   client's ProseMirror state, because y-prosemirror does not write back
 *   while it applies a remote change; each client then writes its own copy of
 *   the repair on its next keystroke anywhere in the document, and the cells
 *   are duplicated. So the repair skips remote batches, and a ragged table is
 *   repaired by the next local edit to that table.
 * - y-prosemirror restores a text, node or all selection after a remote
 *   change, so a cell selection would collapse whenever a collaborator types.
 *   It is rebuilt from its corner cells, kept as Yjs relative positions.
 */
import { Plugin, Selection, TextSelection } from 'prosemirror-state'
import type { Command, EditorState, Transaction } from 'prosemirror-state'
import { Fragment } from 'prosemirror-model'
import type { Node as ProseMirrorNode, ResolvedPos, Slice } from 'prosemirror-model'
import type { EditorView } from 'prosemirror-view'
import { keymap } from 'prosemirror-keymap'
import { chainCommands } from 'prosemirror-commands'
import { GapCursor, gapCursor } from 'prosemirror-gapcursor'
import {
  CellSelection,
  TableMap,
  TableView,
  cellAround,
  columnResizing,
  columnResizingPluginKey,
  deleteCellSelection,
  goToNextCell,
  inSameTable,
  isInTable,
  pointsAtCell,
  tableEditing,
} from 'prosemirror-tables'
import { absolutePositionToRelativePosition, relativePositionToAbsolutePosition, ySyncPluginKey } from 'y-prosemirror'
import type { ProsemirrorBinding } from 'y-prosemirror'
import { tablesAllowed } from './tablesAllowed'
import { cellsToTsv, textToTableSlice, tidyPastedTableHtml } from './tableClipboard'
import {
  addRowBelowAndEnter,
  deleteSelectedTable,
  emptyRow,
  nextCellOrNewRow,
  placeTable,
  resetColumnWidthAtHandle,
} from './tableCommands'

/** Narrowest a column can be dragged, and the floor for unsized columns. */
export const MIN_COLUMN_WIDTH = 48

/** Whether `tr` is y-prosemirror applying a change from the shared document. */
export function isRemoteChange(tr: Transaction): boolean {
  return (tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined)?.isChangeOrigin === true
}

/**
 * prosemirror-tables' `tableEditing`, with its repair and selection
 * normalisation skipped for any batch holding a remote change.
 */
export function collabTableEditing(): Plugin {
  const base = tableEditing({ allowTableNodeSelection: true })
  const append = base.spec.appendTransaction!
  return new Plugin({
    ...base.spec,
    appendTransaction(trs, oldState, state) {
      if (trs.some(isRemoteChange)) return null
      return append.call(this, trs, oldState, state)
    },
  })
}

type Binding = Pick<ProsemirrorBinding, 'doc' | 'type' | 'mapping'>
type RelativePosition = ReturnType<typeof absolutePositionToRelativePosition>

function bindingOf(state: EditorState): Binding | null {
  return (ySyncPluginKey.getState(state) as { binding?: Binding } | undefined)?.binding ?? null
}

/**
 * Keeps a cell selection on its cells while collaborators edit. y-prosemirror
 * remembers a selection by its anchor and head, which for a cell selection are
 * the edges of the first cell's content, not its corner cells. So this plugin
 * keeps the corner cells itself, as Yjs relative positions taken once
 * y-prosemirror has written the document they belong to.
 */
export function cellSelectionRestore(): Plugin {
  let saved: { selection: CellSelection; anchor: RelativePosition; head: RelativePosition } | null = null
  return new Plugin({
    view: () => ({
      update(view) {
        const { selection } = view.state
        if (!(selection instanceof CellSelection)) {
          saved = null
          return
        }
        const binding = bindingOf(view.state)
        if (saved?.selection === selection || !binding) return
        saved = {
          selection,
          anchor: absolutePositionToRelativePosition(selection.$anchorCell.pos, binding.type, binding.mapping),
          head: absolutePositionToRelativePosition(selection.$headCell.pos, binding.type, binding.mapping),
        }
      },
    }),
    appendTransaction(trs, oldState, state) {
      if (!saved || oldState.selection !== saved.selection || state.selection instanceof CellSelection) return null
      if (!trs.some(isRemoteChange)) return null
      const binding = bindingOf(state)
      if (!binding) return null
      const absolute = (pos: RelativePosition) =>
        relativePositionToAbsolutePosition(binding.doc, binding.type, pos, binding.mapping)
      const anchor = absolute(saved.anchor)
      const head = absolute(saved.head)
      if (anchor === null || head === null) return null
      if (anchor > state.doc.content.size || head > state.doc.content.size) return null
      const $anchor = state.doc.resolve(anchor)
      const $head = state.doc.resolve(head)
      if (!pointsAtCell($anchor) || !pointsAtCell($head) || !inSameTable($anchor, $head)) return null
      return state.tr.setSelection(new CellSelection($anchor, $head))
    },
  })
}

/**
 * The cells a typed `| a | b |` line describes, as inline content with marks
 * kept, or null. Only plain text lines qualify: a line holding a mention or
 * another inline node is left alone.
 */
export function pipeRowCells(paragraph: ProseMirrorNode): Fragment[] | null {
  let textOnly = true
  paragraph.forEach((node) => {
    if (!node.isText) textOnly = false
  })
  if (!textOnly) return null
  const text = paragraph.textContent
  if (!/^\s*\|.*\|\s*$/.test(text)) return null
  const pipes: number[] = []
  for (let i = 0; i < text.length; i++) if (text[i] === '|') pipes.push(i)
  if (pipes.length < 3) return null
  const cells: Fragment[] = []
  for (let i = 0; i + 1 < pipes.length; i++) {
    const start = pipes[i]! + 1
    const end = pipes[i + 1]!
    const value = text.slice(start, end)
    const lead = value.length - value.trimStart().length
    const trail = value.length - value.trimEnd().length
    cells.push(start + lead < end - trail ? paragraph.content.cut(start + lead, end - trail) : Fragment.empty)
  }
  return cells
}

/**
 * Enter at the end of a top-level `| Name | Owner |` line: a table with that
 * header row and one empty row, caret in the empty row's first cell.
 */
export function pipeRowToTable(docId: () => string): Command {
  return (state, dispatch) => {
    const { selection, schema } = state
    const { $from } = selection
    if (!selection.empty || $from.depth !== 1) return false
    const paragraph = $from.parent
    if (paragraph.type !== schema.nodes.paragraph || $from.parentOffset !== paragraph.content.size) return false
    const cells = pipeRowCells(paragraph)
    if (!cells || !tablesAllowed(docId())) return false
    if (dispatch) {
      const header = schema.nodes.table_row!.create(
        null,
        cells.map((content) => schema.nodes.table_header!.create(null, schema.nodes.paragraph!.create(null, content))),
      )
      const table = schema.nodes.table!.create(null, [header, emptyRow(state, cells.length)])
      dispatch(placeTable(state, table, { caret: [1, 0], replaceBlock: true }))
    }
    return true
  }
}

// Untyped in prosemirror-gapcursor, but public on the class.
const gapCursorFits = (GapCursor as unknown as { valid($pos: ResolvedPos): boolean }).valid

/**
 * Up from the first row or Down from the last: out of the table, onto the
 * block beyond it, or a gap cursor where there is no textblock to land in.
 * Without this, prosemirror-tables leaves only from the last column (where
 * the gap cursor finds the edge) and otherwise falls back into the table.
 */
export function leaveTable(dir: -1 | 1): Command {
  return (state, dispatch, view) => {
    const { selection } = state
    if (!(selection instanceof TextSelection) || !view?.endOfTextblock(dir < 0 ? 'up' : 'down')) return false
    const $cell = cellAround(selection.$head)
    if (!$cell) return false
    const map = TableMap.get($cell.node(-1))
    const rect = map.findCell($cell.pos - $cell.start(-1))
    if (dir < 0 ? rect.top > 0 : rect.bottom < map.height) return false
    const $out = state.doc.resolve(dir < 0 ? $cell.before(-1) : $cell.after(-1))
    const target = gapCursorFits($out) ? new GapCursor($out) : Selection.near($out, dir)
    dispatch?.(state.tr.setSelection(target).scrollIntoView())
    return true
  }
}

/** Swallow a key inside a table even when `command` can't move further. */
function inTable(command: Command): Command {
  return (state, dispatch, view) => {
    if (!isInTable(state)) return false
    command(state, dispatch, view)
    return true
  }
}

/**
 * The table keymap. It goes after the list keymap, so Tab in a list inside a
 * cell indents the item, and after the editor's own Mod-Enter, so a code
 * block in a cell still exits on it; and before `baseKeymap`.
 */
export function tableKeymap(docId: () => string): Plugin {
  const deleteCells = chainCommands(deleteSelectedTable, deleteCellSelection)
  return keymap({
    Tab: nextCellOrNewRow,
    'Shift-Tab': inTable(goToNextCell(-1)),
    'Mod-Enter': addRowBelowAndEnter,
    ArrowUp: leaveTable(-1),
    ArrowDown: leaveTable(1),
    Enter: pipeRowToTable(docId),
    Backspace: deleteCells,
    'Mod-Backspace': deleteCells,
    Delete: deleteCells,
    'Mod-Delete': deleteCells,
  })
}

/**
 * Plain text pasted as a table where tables are allowed (GFM pipe tables and
 * tab-separated lines), pasted HTML tables tidied to fit the schema, and
 * copied cells as tab-separated text.
 */
export function tableClipboard(docId: () => string): Plugin {
  return new Plugin({
    props: {
      transformPastedHTML: tidyPastedTableHtml,
      clipboardTextParser(text, _$context, plain, view) {
        if (plain || !tablesAllowed(docId())) return null as unknown as Slice
        return textToTableSlice(view.state.schema, text) as Slice
      },
      clipboardTextSerializer(slice, view) {
        if (!(view.state.selection instanceof CellSelection)) return ''
        return cellsToTsv(view.state.schema, slice) ?? ''
      },
    },
  })
}

/** prosemirror-tables' table view, in the same wrapper `toDOM` renders. */
class ScrollingTableView extends TableView {
  constructor(node: ProseMirrorNode, defaultCellMinWidth: number) {
    super(node, defaultCellMinWidth)
    this.dom.className = 'table-scroll'
  }
}

/** Double-clicking a column border clears that column's width. */
function resetColumnOnDoubleClick(): Plugin {
  return new Plugin({
    props: {
      handleDOMEvents: {
        dblclick(view: EditorView, event: MouseEvent) {
          const handle = (columnResizingPluginKey.getState(view.state) as { activeHandle: number } | undefined)
            ?.activeHandle
          if (handle === undefined || handle < 0 || !view.editable) return false
          const tr = resetColumnWidthAtHandle(view.state, handle)
          if (tr) view.dispatch(tr)
          event.preventDefault()
          return true
        },
      },
    },
  })
}

/** Dragging column borders, for pointer devices. */
export function tableColumnResizing(): Plugin[] {
  return [
    columnResizing({
      cellMinWidth: MIN_COLUMN_WIDTH,
      defaultCellMinWidth: MIN_COLUMN_WIDTH,
      View: ScrollingTableView,
    }),
    resetColumnOnDoubleClick(),
  ]
}

/** Whether this device's main pointer is precise enough to drag a border. */
export function hasFinePointer(): boolean {
  return typeof window !== 'undefined' && window.matchMedia?.('(pointer: fine)').matches === true
}

export interface TablePluginOptions {
  /** The collaborative document's id, for `tablesAllowed`. */
  docId: () => string
  /** Whether columns can be resized by dragging (pointer devices). */
  resizable: boolean
}

/**
 * The editor's table plugins in two parts: `keymap` goes after the list
 * keymap and mention plugins and before `baseKeymap`; `plugins` go at the
 * end of the list, where the gap cursor and column resizing get mouse and
 * arrow events before cell selection does.
 */
export function createTablePlugins({ docId, resizable }: TablePluginOptions): {
  keymap: Plugin
  plugins: Plugin[]
} {
  return {
    keymap: tableKeymap(docId),
    plugins: [
      tableClipboard(docId),
      cellSelectionRestore(),
      gapCursor(),
      ...(resizable ? tableColumnResizing() : []),
      collabTableEditing(),
    ],
  }
}
