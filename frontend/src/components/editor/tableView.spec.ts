import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import { CellSelection, TableMap } from 'prosemirror-tables'
import { schema } from './schema'
import { allowTables, resetTablesAllowed } from './tablesAllowed'
import { createTableGuardPlugin } from './tableGuard'
import { createTablePlugins } from './tablePlugins'
import { TableNodeView, createTableControlsPlugin } from './tableView'
import type { TableMenuKind } from './tableView'
import type { TableUiState } from './tableMenus'

const DOC_ID = 'ws-1_doc-1'

const p = (text = '') => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const td = (text: string) => schema.nodes.table_cell.create(null, p(text))
const th = (text: string) => schema.nodes.table_header.create(null, p(text))
const tr = (...cells: Node[]) => schema.nodes.table_row.create(null, cells)
const table = (...rows: Node[]) => schema.nodes.table.create(null, rows)
const doc = (...blocks: Node[]) => schema.nodes.doc.create(null, blocks)
const grid = () => table(tr(th('Name'), th('Owner')), tr(td('VPN'), td('Ana')), tr(td('Mail'), td('Bo')))
const TABLE_POS = 3

let views: EditorView[] = []
beforeEach(() => allowTables(DOC_ID))
afterEach(() => {
  views.forEach((v) => v.destroy())
  views = []
  document.body.innerHTML = ''
  resetTablesAllowed()
})

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))

function open(
  start: Node,
  { editable = true, touch = false }: { editable?: boolean; touch?: boolean } = {},
): { view: EditorView; openMenu: ReturnType<typeof vi.fn>; states: (TableUiState | null)[] } {
  const openMenu = vi.fn((_kind: TableMenuKind, _index: number, _anchor: HTMLElement) => {})
  const states: (TableUiState | null)[] = []
  const tables = createTablePlugins({ docId: () => DOC_ID, resizable: false })
  const mount = document.createElement('div')
  document.body.append(mount)
  const view = new EditorView(mount, {
    state: EditorState.create({
      doc: start,
      plugins: [createTableGuardPlugin(() => DOC_ID), tables.keymap, createTableControlsPlugin((s) => states.push(s)), ...tables.plugins],
    }),
    editable: () => editable,
    nodeViews: {
      table: (node, v, getPos) => new TableNodeView(node, v, getPos, { t: (key) => key, touch, openMenu }),
    },
  })
  views.push(view)
  return { view, openMenu, states }
}

function caret(view: EditorView, row: number, col: number) {
  const tableNode = view.state.doc.nodeAt(TABLE_POS)!
  const map = TableMap.get(tableNode)
  const pos = TABLE_POS + 1 + map.map[row * map.width + col]! + 2
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)))
}

const wrap = (view: EditorView) => view.dom.querySelector('.pm-table') as HTMLElement

describe('the table view', () => {
  it('renders the resting table inside a scroller, with the controls beside it', () => {
    const { view } = open(doc(p('x'), grid()))
    const el = wrap(view)
    expect(el.querySelector(':scope > .table-scroll > table > colgroup + tbody')).not.toBeNull()
    expect(el.querySelectorAll('tbody tr')).toHaveLength(3)
    const ui = el.querySelector(':scope > .pm-table__ui') as HTMLElement
    expect(ui.getAttribute('aria-hidden')).toBe('true')
    expect(ui.contentEditable).toBe('false')
    expect(el.querySelector('.pm-table__corner')!.getAttribute('aria-label')).toBe('editor-table-actions')
  })

  it('pools a handle per row and column once laid out', async () => {
    const { view } = open(doc(p('x'), grid()))
    await nextFrame()
    expect(wrap(view).querySelectorAll('.pm-table__handle--row')).toHaveLength(3)
    expect(wrap(view).querySelectorAll('.pm-table__handle--col')).toHaveLength(2)
    expect(wrap(view).querySelectorAll('.pm-table__insert--row')).toHaveLength(2)
    expect(wrap(view).querySelector('.pm-table__handle--row')!.getAttribute('aria-label')).toBe('editor-table-row-actions')
  })

  it('marks the caret row and column active, and selected ones', async () => {
    const { view, states } = open(doc(p('x'), grid()))
    await nextFrame()
    caret(view, 2, 1)
    expect(wrap(view).classList.contains('is-active')).toBe(true)
    const rows = wrap(view).querySelectorAll('.pm-table__handle--row')
    const cols = wrap(view).querySelectorAll('.pm-table__handle--col')
    expect(rows[2]!.classList.contains('is-active')).toBe(true)
    expect(cols[1]!.classList.contains('is-active')).toBe(true)
    expect(rows[1]!.classList.contains('is-active')).toBe(false)
    expect(states.at(-1)).toMatchObject({ tablePos: TABLE_POS, row: 2, col: 1 })

    const map = TableMap.get(view.state.doc.nodeAt(TABLE_POS)!)
    const at = (r: number, c: number) => TABLE_POS + 1 + map.map[r * map.width + c]!
    view.dispatch(view.state.tr.setSelection(CellSelection.create(view.state.doc, at(0, 0), at(2, 1))))
    expect(wrap(view).classList.contains('is-all-selected')).toBe(true)
    expect(Array.from(rows).every((h) => h.classList.contains('is-selected'))).toBe(true)
  })

  it('goes idle when the caret leaves the table', () => {
    const { view, states } = open(doc(p('x'), grid()))
    caret(view, 1, 1)
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 1)))
    expect(wrap(view).classList.contains('is-active')).toBe(false)
    expect(states.at(-1)).toBeNull()
  })

  it('reports nothing and stays idle read only', () => {
    const { view, states } = open(doc(p('x'), grid()), { editable: false })
    caret(view, 1, 1)
    expect(wrap(view).classList.contains('is-active')).toBe(false)
    expect(states.every((s) => s === null)).toBe(true)
  })

  it('selects every cell and opens the table menu from the corner', () => {
    const { view, openMenu } = open(doc(p('x'), grid()))
    const corner = wrap(view).querySelector('.pm-table__corner') as HTMLElement
    corner.click()
    const sel = view.state.selection as CellSelection
    expect(sel).toBeInstanceOf(CellSelection)
    expect(sel.isRowSelection() && sel.isColSelection()).toBe(true)
    expect(openMenu).toHaveBeenCalledWith('table', 0, corner)
  })

  it('selects a row and opens its menu from its handle', async () => {
    const { view, openMenu } = open(doc(p('x'), grid()))
    await nextFrame()
    const handle = wrap(view).querySelectorAll('.pm-table__handle--row')[1] as HTMLElement
    handle.dispatchEvent(new PointerEvent('pointerdown', { button: 0, bubbles: true }))
    handle.dispatchEvent(new PointerEvent('pointerup', { button: 0, bubbles: true }))
    const sel = view.state.selection as CellSelection
    expect(sel.isRowSelection()).toBe(true)
    expect(sel.$anchorCell.index()).toBe(0)
    expect(openMenu).toHaveBeenCalledWith('row', 1, handle)
  })

  it('adds a row and a column from the add bars', () => {
    const { view } = open(doc(p('x'), grid()))
    ;(wrap(view).querySelector('.pm-table__addbar--row') as HTMLElement).click()
    expect(view.state.doc.nodeAt(TABLE_POS)!.childCount).toBe(4)
    ;(wrap(view).querySelector('.pm-table__addbar--col') as HTMLElement).click()
    expect(TableMap.get(view.state.doc.nodeAt(TABLE_POS)!).width).toBe(3)
  })

  it('inserts between two rows from the boundary plus', async () => {
    const { view } = open(doc(p('x'), grid()))
    await nextFrame()
    ;(wrap(view).querySelectorAll('.pm-table__insert--row')[0] as HTMLElement).click()
    const rows: string[] = []
    view.state.doc.nodeAt(TABLE_POS)!.forEach((row) => rows.push(row.textContent))
    expect(rows).toEqual(['NameOwner', '', 'VPNAna', 'MailBo'])
  })

  it('marks the touch layout', () => {
    const { view } = open(doc(p('x'), grid()), { touch: true })
    expect(wrap(view).classList.contains('is-touch')).toBe(true)
  })
})
