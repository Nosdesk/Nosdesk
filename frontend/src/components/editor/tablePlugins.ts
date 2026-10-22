/**
 * Table editing in the collaborative editor: cell selection, the keyboard,
 * column resizing, the `|` shortcut, and tables through the clipboard.
 *
 * Three of these exist because of y-prosemirror rather than tables:
 *
 * - prosemirror-tables repairs ragged tables (`fixTables`) in an appended
 *   transaction. Run after a remote change, that repair stays in this
 *   client's ProseMirror state, because y-prosemirror does not write back
 *   while it applies a remote change; each client then writes its own copy of
 *   the repair on its next keystroke anywhere in the document, and the cells
 *   are duplicated. So `collabTableEditing` leaves repairs out.
 * - `tableRepair` makes them instead, once the change that left a table
 *   ragged (one person adds a row while another adds a column) has reached
 *   Yjs, as a local transaction that y-prosemirror writes back. Cells added
 *   concurrently by two clients are both kept, so only one client pads a
 *   short row: the one that created it, or, when that client has left the
 *   document, the present client with the lowest id. Every client works that
 *   out from the same shared document, so they agree without talking.
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
  cellAround,
  columnResizing,
  columnResizingPluginKey,
  deleteCellSelection,
  fixTables,
  fixTablesKey,
  goToNextCell,
  inSameTable,
  isInTable,
  pointsAtCell,
  removeColSpan,
  tableEditing,
  tableNodeTypes,
} from 'prosemirror-tables'
import { absolutePositionToRelativePosition, relativePositionToAbsolutePosition, ySyncPluginKey } from 'y-prosemirror'
import type { ProsemirrorBinding } from 'y-prosemirror'
import type * as Y from 'yjs'
import { tablesAllowed } from './tablesAllowed'
import { cellsToTsv, textToTableSlice, tidyPastedTableHtml } from './tableClipboard'
import {
  addRowBelowAndEnter,
  deleteSelectedCellsTable,
  deleteSelectedTable,
  emptyRow,
  nextCellOrNewRow,
  placeTable,
  resetColumnWidthAtHandle,
  selectAllInTable,
} from './tableCommands'

/** Narrowest a column can be dragged, and the floor for unsized columns. */
export const MIN_COLUMN_WIDTH = 48

/** Whether `tr` is y-prosemirror applying a change from the shared document. */
export function isRemoteChange(tr: Transaction): boolean {
  return (tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined)?.isChangeOrigin === true
}

/**
 * prosemirror-tables' `tableEditing`, with its selection normalisation
 * skipped for any batch holding a remote change, and its repair left to
 * `tableRepair` wherever the document is shared. Without a Yjs binding the
 * repair runs as usual.
 */
export function collabTableEditing(): Plugin {
  const base = tableEditing({ allowTableNodeSelection: true })
  const append = base.spec.appendTransaction!
  return new Plugin({
    ...base.spec,
    appendTransaction(trs, oldState, state) {
      if (trs.some(isRemoteChange)) return null
      // With the old state equal to the new one, `fixTables` finds nothing
      // changed, so only the selection is normalised.
      return append.call(this, trs, bindingOf(state) ? state : oldState, state)
    },
  })
}

type Binding = Pick<ProsemirrorBinding, 'doc' | 'type' | 'mapping'>
type RelativePosition = ReturnType<typeof absolutePositionToRelativePosition>

function bindingOf(state: EditorState): Binding | null {
  return (ySyncPluginKey.getState(state) as { binding?: Binding } | undefined)?.binding ?? null
}

function isRagged(doc: ProseMirrorNode): boolean {
  let ragged = false
  doc.forEach((node) => {
    if (!ragged && node.type.spec.tableRole === 'table' && TableMap.get(node).problems) ragged = true
  })
  return ragged
}

/**
 * Who created each row of `table`, from its Yjs element, or null when this
 * client's ProseMirror and Yjs documents do not line up for it yet.
 */
function rowCreators(binding: Binding, table: ProseMirrorNode): number[] | null {
  let yTable: Y.XmlElement | null = null
  binding.type.forEach((child) => {
    if (!yTable && binding.mapping.get(child) === table) yTable = child as Y.XmlElement
  })
  const rows = (yTable as Y.XmlElement | null)?.toArray()
  if (!rows || rows.length !== table.childCount) return null
  const creators: number[] = []
  for (let i = 0; i < rows.length; i++) {
    const client = rows[i]!._item?.id.client
    if (binding.mapping.get(rows[i]!) !== table.child(i) || client === undefined) return null
    creators.push(client)
  }
  return creators
}

/**
 * prosemirror-tables' `fixTable`, except that cells are added only to the
 * rows `padsRow` accepts. The attribute repairs are the same whichever
 * client makes them, so every client makes those.
 */
function repairTable(
  state: EditorState,
  table: ProseMirrorNode,
  tablePos: number,
  tr: Transaction,
  padsRow: (row: number) => boolean,
): void {
  const map = TableMap.get(table)
  if (!map.problems) return
  const mustAdd: number[] = Array.from({ length: map.height }, () => 0)
  for (const prob of map.problems) {
    if (prob.type === 'collision') {
      const cell = table.nodeAt(prob.pos)
      if (!cell) continue
      const attrs = cell.attrs as Parameters<typeof removeColSpan>[0]
      for (let j = 0; j < attrs.rowspan; j++) mustAdd[prob.row + j]! += prob.n
      const fixed = removeColSpan(attrs, attrs.colspan - prob.n, prob.n)
      tr.setNodeMarkup(tr.mapping.map(tablePos + 1 + prob.pos), null, fixed)
    } else if (prob.type === 'missing') {
      mustAdd[prob.row]! += prob.n
    } else if (prob.type === 'overlong_rowspan') {
      const cell = table.nodeAt(prob.pos)
      if (!cell) continue
      const rowspan = cell.attrs.rowspan - prob.n
      tr.setNodeMarkup(tr.mapping.map(tablePos + 1 + prob.pos), null, { ...cell.attrs, rowspan })
    } else if (prob.type === 'colwidth mismatch') {
      const cell = table.nodeAt(prob.pos)
      if (!cell) continue
      tr.setNodeMarkup(tr.mapping.map(tablePos + 1 + prob.pos), null, { ...cell.attrs, colwidth: prob.colwidth })
    } else if (prob.type === 'zero_sized') {
      const pos = tr.mapping.map(tablePos)
      tr.delete(pos, pos + table.nodeSize)
      return
    }
  }
  let first: number | undefined
  let last: number | undefined
  mustAdd.forEach((add, i) => {
    if (!add) return
    first ??= i
    last = i
  })
  for (let i = 0, pos = tablePos + 1; i < map.height; i++) {
    const row = table.child(i)
    const end = pos + row.nodeSize
    const add = mustAdd[i]!
    if (add > 0 && padsRow(i)) {
      const role = (row.firstChild?.type.spec.tableRole ?? 'cell') as 'cell' | 'header_cell'
      const cells: ProseMirrorNode[] = []
      for (let j = 0; j < add; j++) {
        const cell = tableNodeTypes(state.schema)[role].createAndFill()
        if (cell) cells.push(cell)
      }
      const side = (i === 0 || first === i - 1) && last === i ? pos + 1 : end - 1
      tr.insert(tr.mapping.map(side), cells)
    }
    pos = end
  }
}

/**
 * The repair for every ragged table in `state`, with cells added only to the
 * short rows this client pads (see the file comment), or null.
 * `present` lists the clients in the document; without it, a row is padded
 * by its creator alone.
 */
export function tableRepairTransaction(state: EditorState, present?: Iterable<number>): Transaction | null {
  const binding = bindingOf(state)
  if (!binding) {
    const tr = fixTables(state)
    return tr?.docChanged ? tr : null
  }
  const me = binding.doc.clientID
  const here = present ? new Set(present) : null
  here?.add(me)
  const lowest = here ? Math.min(...here) : me
  const pads = (creator: number) => (!here || here.has(creator) ? creator === me : lowest === me)
  const tr = state.tr
  state.doc.forEach((table, pos) => {
    if (table.type.spec.tableRole !== 'table' || !TableMap.get(table).problems) return
    const creators = rowCreators(binding, table)
    repairTable(state, table, pos, tr, (row) => creators !== null && pads(creators[row]!))
  })
  return tr.docChanged ? tr : null
}

export interface TableRepairOptions {
  /** Yjs client ids of everyone in the document (awareness), this one included. */
  present?: () => Iterable<number>
}

/**
 * Repairs ragged tables after a change has reached Yjs, local or remote, as
 * a transaction of its own that y-prosemirror writes back. It waits for a
 * microtask because y-prosemirror ignores a transaction dispatched while it
 * applies a remote change. The repair stays out of undo history, and is
 * skipped while the editor is read only or showing a revision.
 */
export function tableRepair({ present }: TableRepairOptions = {}): Plugin {
  return new Plugin({
    view(view) {
      let pending = false
      // A repair is not checked again until the next change, so one that
      // leaves rows for other clients to pad never loops.
      let repairing = false
      const repair = () => {
        pending = false
        if (view.isDestroyed || !view.editable) return
        const sync = ySyncPluginKey.getState(view.state) as { snapshot?: unknown; prevSnapshot?: unknown } | undefined
        if (sync?.snapshot != null || sync?.prevSnapshot != null) return
        const tr = tableRepairTransaction(view.state, present?.())
        if (!tr) return
        repairing = true
        try {
          view.dispatch(tr.setMeta(fixTablesKey, { fixTables: true }).setMeta('addToHistory', false))
        } finally {
          repairing = false
        }
      }
      const check = (doc: ProseMirrorNode) => {
        if (pending || repairing || !isRagged(doc)) return
        pending = true
        queueMicrotask(repair)
      }
      check(view.state.doc)
      return {
        update(updated, prevState) {
          if (updated.state.doc !== prevState.doc) check(updated.state.doc)
        },
      }
    },
  })
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

/** `command`, then `after` once it has applied. */
function andThen(command: Command, after: (() => void) | undefined): Command {
  return (state, dispatch, view) => {
    if (!command(state, dispatch, view)) return false
    if (dispatch) after?.()
    return true
  }
}

/**
 * The table keymap. It goes after the list keymap, so Tab in a list inside a
 * cell indents the item, and after the editor's own Mod-Enter, so a code
 * block in a cell still exits on it; and before `baseKeymap`.
 * `onTableDeleted` runs after Backspace or Delete removes a whole table.
 */
export function tableKeymap(docId: () => string, onTableDeleted?: () => void): Plugin {
  const deleteCells = chainCommands(
    andThen(chainCommands(deleteSelectedTable, deleteSelectedCellsTable), onTableDeleted),
    deleteCellSelection,
  )
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
    'Mod-a': selectAllInTable,
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
      // The editor's own table view (tableView.ts) draws the table.
      View: null,
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
  /** Runs after Backspace or Delete removes a whole table. */
  onTableDeleted?: () => void
  /** Yjs client ids of everyone in the document, for `tableRepair`. */
  present?: () => Iterable<number>
}

/**
 * The editor's table plugins in two parts: `keymap` goes after the list
 * keymap and mention plugins and before `baseKeymap`; `plugins` go at the
 * end of the list, where the gap cursor and column resizing get mouse and
 * arrow events before cell selection does.
 */
export function createTablePlugins({ docId, resizable, onTableDeleted, present }: TablePluginOptions): {
  keymap: Plugin
  plugins: Plugin[]
} {
  return {
    keymap: tableKeymap(docId, onTableDeleted),
    plugins: [
      tableClipboard(docId),
      cellSelectionRestore(),
      gapCursor(),
      ...(resizable ? tableColumnResizing() : []),
      collabTableEditing(),
      tableRepair({ present }),
    ],
  }
}
