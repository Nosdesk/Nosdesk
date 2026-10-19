import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import { CellSelection, TableMap, columnResizingPluginKey } from 'prosemirror-tables'
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
  {
    editable = true,
    touch = false,
    resizable = false,
  }: { editable?: boolean; touch?: boolean; resizable?: boolean } = {},
): { view: EditorView; openMenu: ReturnType<typeof vi.fn>; states: (TableUiState | null)[] } {
  const openMenu = vi.fn((_kind: TableMenuKind, _index: number, _anchor: HTMLElement) => {})
  const states: (TableUiState | null)[] = []
  const tables = createTablePlugins({ docId: () => DOC_ID, resizable })
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

describe('the column resize guide', () => {
  // A 3 by 2 table, 400 wide and 90 tall, at (100, 50); columns split at 300.
  const merged = () =>
    table(
      tr(th('Name'), th('Owner')),
      tr(schema.nodes.table_cell.create({ colspan: 2 }, p('Spans both'))),
      tr(td('Mail'), td('Bo')),
    )

  function rect(left: number, top: number, width: number, height: number): DOMRect {
    return { left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }
  }

  /** Lay the table out at fixed coordinates, the first column `split` wide. */
  function stubLayout(view: EditorView, split: { at: number }) {
    const el = wrap(view)
    el.getBoundingClientRect = () => rect(100, 50, 400, 120)
    el.querySelector('table')!.getBoundingClientRect = () => rect(100, 50, 400, 90)
    for (const cell of Array.from(el.querySelectorAll('td, th')) as HTMLElement[]) {
      const first = cell.previousElementSibling === null
      const spans = cell.getAttribute('colspan') === '2'
      cell.getBoundingClientRect = () =>
        spans ? rect(100, 80, 400, 30) : first ? rect(100, 50, split.at - 100, 30) : rect(split.at, 50, 500 - split.at, 30)
    }
  }

  const cellPos = (view: EditorView, row: number, col: number) => {
    const map = TableMap.get(view.state.doc.nodeAt(TABLE_POS)!)
    return TABLE_POS + 1 + map.map[row * map.width + col]!
  }
  const meta = (view: EditorView, value: object) =>
    view.dispatch(view.state.tr.setMeta(columnResizingPluginKey, value))
  const guide = (view: EditorView) => wrap(view).querySelector('.pm-table__guideclip > .pm-table__guide') as HTMLElement
  const shown = (view: EditorView) => guide(view).classList.contains('is-on')
  const x = (view: EditorView) => Number(/translateX\((-?[\d.]+)px\)/.exec(guide(view).style.transform)![1])

  it('sits beside the controls overlay, hidden at rest', () => {
    const { view } = open(doc(p('x'), grid()), { resizable: true })
    expect(wrap(view).querySelector(':scope > .table-scroll + .pm-table__guideclip + .pm-table__ui')).not.toBeNull()
    expect(shown(view)).toBe(false)
  })

  it('draws one line down the whole table on the hovered border', async () => {
    const { view } = open(doc(p('x'), merged()), { resizable: true })
    stubLayout(view, { at: 300 })
    meta(view, { setHandle: cellPos(view, 2, 0) })
    await nextFrame()
    expect(shown(view)).toBe(true)
    expect(wrap(view).querySelectorAll('.pm-table__guide')).toHaveLength(1)
    // Centred on the border at 300, 200 into the table.
    expect(x(view)).toBe(199)
    expect(guide(view).style.top).toBe('0px')
    expect(guide(view).style.height).toBe('90px')
  })

  it('stays inside the table on its last border', async () => {
    const { view } = open(doc(p('x'), merged()), { resizable: true })
    stubLayout(view, { at: 300 })
    meta(view, { setHandle: cellPos(view, 1, 0) })
    await nextFrame()
    expect(x(view)).toBe(398)
  })

  it('moves with the table as it scrolls sideways', async () => {
    const { view } = open(doc(p('x'), merged()), { resizable: true })
    stubLayout(view, { at: 300 })
    meta(view, { setHandle: cellPos(view, 0, 0) })
    await nextFrame()
    const scroller = wrap(view).querySelector('.table-scroll') as HTMLElement
    scroller.scrollLeft = 50
    scroller.dispatchEvent(new Event('scroll'))
    expect(x(view)).toBe(149)
  })

  it('follows the border while it is dragged and goes on release', async () => {
    const { view } = open(doc(p('x'), merged()), { resizable: true })
    const split = { at: 300 }
    stubLayout(view, split)
    meta(view, { setHandle: cellPos(view, 0, 0) })
    meta(view, { setDragging: { startX: 300, startWidth: 200 } })
    await nextFrame()
    expect(x(view)).toBe(199)

    split.at = 360
    window.dispatchEvent(new MouseEvent('mousemove', { clientX: 360 }))
    await nextFrame()
    expect(shown(view)).toBe(true)
    expect(x(view)).toBe(259)

    meta(view, { setDragging: null })
    expect(shown(view)).toBe(false)
    // Still on the same border: no guide until the pointer finds one again.
    await nextFrame()
    expect(shown(view)).toBe(false)
    meta(view, { setDragging: { startX: 360, startWidth: 260 } })
    await nextFrame()
    expect(shown(view)).toBe(true)
  })

  it('goes when the pointer leaves the border', async () => {
    const { view } = open(doc(p('x'), merged()), { resizable: true })
    stubLayout(view, { at: 300 })
    meta(view, { setHandle: cellPos(view, 0, 0) })
    await nextFrame()
    expect(shown(view)).toBe(true)
    meta(view, { setHandle: -1 })
    expect(shown(view)).toBe(false)
  })

  it('is never drawn read only or on touch', async () => {
    for (const options of [{ editable: false }, { touch: true }]) {
      const { view } = open(doc(p('x'), merged()), { resizable: true, ...options })
      stubLayout(view, { at: 300 })
      meta(view, { setHandle: cellPos(view, 0, 0) })
      await nextFrame()
      expect(shown(view)).toBe(false)
    }
  })
})
