/**
 * The table node view of the live editor, and the plugin that keeps its
 * controls in step with the selection.
 *
 * DOM: `div.pm-table > div.table-scroll > table > (colgroup, tbody)`, the
 * same `.table-scroll > table` that `toDOM` renders for revisions, saved
 * pages and the portal, so one set of resting styles serves them all. Next
 * to the scroller sits an overlay (`.pm-table__ui`) with the controls:
 *
 * - a corner handle that selects every cell and opens the table menu;
 * - a pill per row and per column that selects it and opens its menu, and
 *   a drag across pills that selects several;
 * - a plus on the boundary between two rows or columns (pointer devices);
 * - add bars after the last row and column.
 *
 * Handles belong to the cell you are in: nothing is drawn at rest, a hover
 * shows a faint pill, the caret's row and column a solid one, and a
 * selected row, column or table the accent. On touch the handles show for
 * the caret's cell only and the add bars stay on while the caret is in the
 * table. The overlay is plain DOM rather than Vue: it is rebuilt on every
 * layout and lives inside ProseMirror's node view, which owns its updates.
 *
 * The overlay is hidden by CSS when the editor is read only or the document
 * may not hold tables (`.table-controls-off` on the editor).
 */
import { Plugin, PluginKey, Selection } from 'prosemirror-state'
import type { EditorState } from 'prosemirror-state'
import type { Node as ProseMirrorNode } from 'prosemirror-model'
import type { EditorView, NodeView, ViewMutationRecord } from 'prosemirror-view'
import { TableMap, cellAround, updateColumnsOnResize } from 'prosemirror-tables'
import { ICON_REGISTRY } from '@/components/common/icons'
import { MIN_COLUMN_WIDTH } from './tablePlugins'
import { insertColumnAt, insertRowAt, selectAllCells, selectColumn, selectRow } from './tableCommands'
import { tableUiState } from './tableMenus'
import type { TableUiState } from './tableMenus'

export type TableMenuKind = 'row' | 'column' | 'table'

/** What the node views share with the editor around them. */
export interface TableViewHost {
  t: (key: string, args?: Record<string, string | number>) => string
  /** Touch layout: no hover, handles for the caret's cell, add bars on. */
  touch: boolean
  /** Open the row, column or table menu, anchored on `anchor`. */
  openMenu: (kind: TableMenuKind, index: number, anchor: HTMLElement) => void
}

const SVG_NS = 'http://www.w3.org/2000/svg'

function svgIcon(name: keyof typeof ICON_REGISTRY): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg')
  svg.setAttribute('viewBox', '0 0 24 24')
  svg.setAttribute('fill', 'none')
  svg.setAttribute('stroke', 'currentColor')
  svg.setAttribute('stroke-width', '2')
  svg.setAttribute('stroke-linecap', 'round')
  svg.setAttribute('stroke-linejoin', 'round')
  svg.setAttribute('aria-hidden', 'true')
  const path = document.createElementNS(SVG_NS, 'path')
  path.setAttribute('d', ICON_REGISTRY[name].d)
  svg.append(path)
  return svg
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag)
  if (className) el.className = className
  return el
}

function controlButton(className: string, title: string, icon?: keyof typeof ICON_REGISTRY): HTMLButtonElement {
  const button = element('button', className)
  button.type = 'button'
  button.tabIndex = -1
  button.title = title
  button.setAttribute('aria-label', title)
  if (icon) button.append(svgIcon(icon))
  // Keep the caret (and the phone's keyboard) where it is.
  button.addEventListener('mousedown', (e) => e.preventDefault())
  return button
}

const IDLE: TableUiState = { tablePos: -1, row: -1, col: -1, rows: [], cols: [], all: false, cells: false }

/** Node views by editor, so the plugin can reach those of its own view. */
const registries = new WeakMap<EditorView, Set<TableNodeView>>()

function registryOf(view: EditorView): Set<TableNodeView> {
  let set = registries.get(view)
  if (!set) registries.set(view, (set = new Set()))
  return set
}

export class TableNodeView implements NodeView {
  dom: HTMLDivElement
  contentDOM: HTMLTableSectionElement
  readonly scroll: HTMLDivElement
  private table: HTMLTableElement
  private colgroup: HTMLTableColElement
  private ui: HTMLDivElement
  private corner: HTMLButtonElement
  private colstrip: HTMLDivElement
  private rowstrip: HTMLDivElement
  private addColumn: HTMLButtonElement
  private addRow: HTMLButtonElement
  private colHandles: HTMLButtonElement[] = []
  private rowHandles: HTMLButtonElement[] = []
  private colInserts: HTMLButtonElement[] = []
  private rowInserts: HTMLButtonElement[] = []
  private state: TableUiState = IDLE
  private hoverRow = -1
  private hoverCol = -1
  private frame = 0
  private resizeObserver: ResizeObserver | null = null

  constructor(
    private node: ProseMirrorNode,
    private view: EditorView,
    private getPos: () => number | undefined,
    private host: TableViewHost,
  ) {
    const { t } = host
    this.dom = element('div', 'pm-table')
    if (host.touch) this.dom.classList.add('is-touch')
    this.scroll = element('div', 'table-scroll')
    this.table = element('table')
    this.colgroup = element('colgroup')
    this.contentDOM = element('tbody')
    this.table.append(this.colgroup, this.contentDOM)
    this.scroll.append(this.table)

    // Pointer shortcuts only: the toolbar's Table button reaches every
    // action from the keyboard and for assistive tech.
    this.ui = element('div', 'pm-table__ui')
    this.ui.contentEditable = 'false'
    this.ui.setAttribute('aria-hidden', 'true')
    this.corner = controlButton('pm-table__corner', t('editor-table-actions'), 'table')
    const colclip = element('div', 'pm-table__colclip')
    this.colstrip = element('div', 'pm-table__colstrip')
    colclip.append(this.colstrip)
    this.rowstrip = element('div', 'pm-table__rowstrip')
    const sideclip = element('div', 'pm-table__sideclip')
    this.addColumn = controlButton('pm-table__addbar pm-table__addbar--col', t('editor-table-add-column'), 'add')
    this.addRow = controlButton('pm-table__addbar pm-table__addbar--row', t('editor-table-add-row'), 'add')
    sideclip.append(this.addColumn)
    this.ui.append(this.corner, colclip, this.rowstrip, sideclip, this.addRow)
    this.dom.append(this.scroll, this.ui)

    updateColumnsOnResize(node, this.colgroup, this.table, MIN_COLUMN_WIDTH)

    this.scroll.addEventListener('scroll', () => this.onScroll(), { passive: true })
    this.dom.addEventListener('mouseenter', () => {
      if (!host.touch) this.dom.classList.add('is-hover')
    })
    this.dom.addEventListener('mouseleave', () => {
      this.dom.classList.remove('is-hover')
      this.setHover(-1, -1)
      this.hideInserts()
    })
    this.scroll.addEventListener('mousemove', (e) => {
      if (host.touch) return
      const cell = (e.target as Element | null)?.closest?.('td, th')
      const at = cell ? this.cellCoords(cell) : null
      if (at) this.setHover(at.top, at.left)
    })
    this.colstrip.addEventListener('mousemove', (e) => this.trackInserts('col', e))
    this.rowstrip.addEventListener('mousemove', (e) => this.trackInserts('row', e))
    this.colstrip.addEventListener('mouseleave', () => this.hideInserts('col'))
    this.rowstrip.addEventListener('mouseleave', () => this.hideInserts('row'))
    this.corner.addEventListener('click', () => {
      const tablePos = this.focusTable()
      if (tablePos === null) return
      selectAllCells(tablePos)(this.view.state, this.view.dispatch)
      host.openMenu('table', 0, this.corner)
    })
    this.addRow.addEventListener('click', () => this.insertAtEnd('row'))
    this.addColumn.addEventListener('click', () => this.insertAtEnd('col'))

    if (typeof ResizeObserver !== 'undefined') {
      this.resizeObserver = new ResizeObserver(() => this.scheduleLayout())
      this.resizeObserver.observe(this.table)
    }
    registryOf(view).add(this)
    this.scheduleLayout()
  }

  /** This table's position, or null once it has left the document. */
  get pos(): number | null {
    const pos = this.getPos()
    return pos === undefined ? null : pos
  }

  /**
   * Put the caret in this table's first cell unless the selection is
   * already in it, and return the table's position.
   */
  private focusTable(): number | null {
    const pos = this.pos
    if (pos === null) return null
    if (tableUiState(this.view.state)?.tablePos !== pos) {
      const { state } = this.view
      this.view.dispatch(state.tr.setSelection(Selection.near(state.doc.resolve(pos + 1), 1)))
    }
    return pos
  }

  private insertAtEnd(kind: 'row' | 'col'): void {
    if (this.focusTable() === null) return
    const map = TableMap.get(this.node)
    const command = kind === 'row' ? insertRowAt(map.height) : insertColumnAt(map.width)
    command(this.view.state, this.view.dispatch)
    this.view.focus()
  }

  private cellCoords(cell: Element): { top: number; left: number } | null {
    const pos = this.pos
    if (pos === null) return null
    try {
      const $cell = cellAround(this.view.state.doc.resolve(this.view.posAtDOM(cell, 0)))
      if (!$cell || $cell.start(-1) !== pos + 1) return null
      return TableMap.get(this.node).findCell($cell.pos - $cell.start(-1))
    } catch {
      return null
    }
  }

  update(node: ProseMirrorNode): boolean {
    if (node.type !== this.node.type) return false
    this.node = node
    updateColumnsOnResize(node, this.colgroup, this.table, MIN_COLUMN_WIDTH)
    this.scheduleLayout()
    return true
  }

  ignoreMutation(record: ViewMutationRecord): boolean {
    // The colgroup and the controls are ours; the cells are ProseMirror's.
    return !this.contentDOM.contains(record.target)
  }

  stopEvent(event: Event): boolean {
    return this.ui.contains(event.target as Node)
  }

  destroy(): void {
    registryOf(this.view).delete(this)
    this.resizeObserver?.disconnect()
    if (this.frame) cancelAnimationFrame(this.frame)
  }

  private onScroll(): void {
    const x = -this.scroll.scrollLeft
    this.colstrip.style.transform = `translateX(${x}px)`
    this.addColumn.style.transform = `translateX(${x}px)`
    this.updateFade()
  }

  private updateFade(): void {
    const max = this.scroll.scrollWidth - this.scroll.clientWidth
    const left = this.scroll.scrollLeft > 2
    const right = max - this.scroll.scrollLeft > 2
    this.dom.dataset.fade = left && right ? 'both' : left ? 'left' : right ? 'right' : 'none'
  }

  scheduleLayout(): void {
    if (this.frame || typeof requestAnimationFrame === 'undefined') return
    this.frame = requestAnimationFrame(() => {
      this.frame = 0
      this.layout()
    })
  }

  /** Size and place the handles from the rendered cells. */
  layout(): void {
    const pos = this.pos
    if (!this.dom.isConnected || pos === null) return
    const { t } = this.host
    const map = TableMap.get(this.node)
    const tableRect = this.table.getBoundingClientRect()
    const tableStart = pos + 1
    // Column edges from a cell spanning exactly that column; a column only
    // spanned shares its spanning cell's width evenly.
    const colL: (number | null)[] = new Array(map.width).fill(null)
    const colR: (number | null)[] = new Array(map.width).fill(null)
    for (let c = 0; c < map.width; c++) {
      for (let r = 0; r < map.height; r++) {
        const cellPos = map.map[r * map.width + c]!
        if ((this.node.nodeAt(cellPos)!.attrs.colspan as number) !== 1) continue
        const dom = this.view.nodeDOM(tableStart + cellPos) as HTMLElement | null
        if (!dom?.getBoundingClientRect) continue
        const box = dom.getBoundingClientRect()
        colL[c] = box.left - tableRect.left
        colR[c] = box.right - tableRect.left
        break
      }
    }
    for (let c = 0; c < map.width; c++) {
      if (colL[c] !== null) continue
      const cellPos = map.map[c]!
      const cell = this.node.nodeAt(cellPos)!
      const dom = this.view.nodeDOM(tableStart + cellPos) as HTMLElement | null
      const box = dom?.getBoundingClientRect ? dom.getBoundingClientRect() : tableRect
      const width = (box.right - box.left) / (cell.attrs.colspan as number)
      colL[c] = box.left - tableRect.left + (c - map.colCount(cellPos)) * width
      colR[c] = colL[c]! + width
    }
    const rows = Array.from(this.contentDOM.children).map((row) => {
      const box = row.getBoundingClientRect()
      return { top: box.top - tableRect.top, height: box.height }
    })
    this.colstrip.style.width = `${tableRect.width}px`
    const pool = <T extends HTMLElement>(list: T[], parent: HTMLElement, n: number, make: () => T) => {
      while (list.length < n) {
        const el = make()
        list.push(el)
        parent.append(el)
      }
      while (list.length > n) list.pop()!.remove()
    }
    pool(this.colHandles, this.colstrip, map.width, () => this.makeHandle('col'))
    pool(this.rowHandles, this.rowstrip, map.height, () => this.makeHandle('row'))
    pool(this.colInserts, this.colstrip, Math.max(0, map.width - 1), () => this.makeInsert('col'))
    pool(this.rowInserts, this.rowstrip, Math.max(0, map.height - 1), () => this.makeInsert('row'))
    this.colHandles.forEach((h, c) => {
      h.style.left = `${colL[c]}px`
      h.style.width = `${colR[c]! - colL[c]!}px`
      h.dataset.index = String(c)
      h.setAttribute('aria-label', t('editor-table-column-actions', { column: c + 1 }))
    })
    this.rowHandles.forEach((h, r) => {
      const row = rows[r] ?? { top: 0, height: 0 }
      h.style.top = `${row.top}px`
      h.style.height = `${row.height}px`
      h.dataset.index = String(r)
      h.setAttribute('aria-label', t('editor-table-row-actions', { row: r + 1 }))
    })
    this.colInserts.forEach((d, i) => {
      d.style.left = `${colL[i + 1]}px`
      d.dataset.index = String(i + 1)
    })
    this.rowInserts.forEach((d, i) => {
      d.style.top = `${rows[i + 1]?.top ?? 0}px`
      d.dataset.index = String(i + 1)
    })
    this.addColumn.style.left = `${tableRect.width + 2}px`
    this.addColumn.style.height = `${tableRect.height}px`
    this.addRow.style.width = `${Math.min(tableRect.width, this.scroll.clientWidth)}px`
    this.onScroll()
    this.applyState()
  }

  private makeHandle(kind: 'row' | 'col'): HTMLButtonElement {
    const handle = element('button', `pm-table__handle pm-table__handle--${kind}`)
    handle.type = 'button'
    handle.tabIndex = -1
    let start = -1
    let moved = false
    let startX = 0
    let startY = 0
    handle.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return
      e.preventDefault()
      start = Number(handle.dataset.index)
      moved = false
      startX = e.clientX
      startY = e.clientY
      handle.setPointerCapture?.(e.pointerId)
    })
    handle.addEventListener('pointermove', (e) => {
      if (start < 0) return
      if (Math.abs(e.clientX - startX) + Math.abs(e.clientY - startY) > 4) moved = true
      if (!moved) return
      const target = document.elementFromPoint?.(e.clientX, e.clientY)
      const other = target?.closest?.(`.pm-table__handle--${kind}`) as HTMLElement | null
      if (!other || !this.ui.contains(other)) return
      const tablePos = this.focusTable()
      if (tablePos === null) return
      const index = Number(other.dataset.index)
      const select = kind === 'row' ? selectRow(tablePos, start, index) : selectColumn(tablePos, start, index)
      select(this.view.state, this.view.dispatch)
    })
    handle.addEventListener('pointerup', () => {
      if (start < 0) return
      const index = start
      start = -1
      if (moved) return
      const tablePos = this.focusTable()
      if (tablePos === null) return
      const select = kind === 'row' ? selectRow(tablePos, index) : selectColumn(tablePos, index)
      select(this.view.state, this.view.dispatch)
      this.host.openMenu(kind === 'row' ? 'row' : 'column', index, handle)
    })
    handle.addEventListener('pointercancel', () => {
      start = -1
    })
    return handle
  }

  private makeInsert(kind: 'row' | 'col'): HTMLButtonElement {
    const label = this.host.t(kind === 'col' ? 'editor-table-insert-column-here' : 'editor-table-insert-row-here')
    const button = controlButton(`pm-table__insert pm-table__insert--${kind}`, label, 'add')
    button.addEventListener('click', () => {
      if (this.focusTable() === null) return
      const index = Number(button.dataset.index)
      const command = kind === 'col' ? insertColumnAt(index) : insertRowAt(index)
      command(this.view.state, this.view.dispatch)
      this.hideInserts()
      this.view.focus()
    })
    return button
  }

  private trackInserts(kind: 'row' | 'col', e: MouseEvent): void {
    if (this.host.touch) return
    const list = kind === 'col' ? this.colInserts : this.rowInserts
    const strip = (kind === 'col' ? this.colstrip : this.rowstrip).getBoundingClientRect()
    const at = kind === 'col' ? e.clientX - strip.left : e.clientY - strip.top
    for (const d of list) {
      const edge = parseFloat(kind === 'col' ? d.style.left : d.style.top)
      d.classList.toggle('is-on', Math.abs(at - edge) < 9)
    }
  }

  private hideInserts(kind?: 'row' | 'col'): void {
    if (kind !== 'row') this.colInserts.forEach((d) => d.classList.remove('is-on'))
    if (kind !== 'col') this.rowInserts.forEach((d) => d.classList.remove('is-on'))
  }

  private setHover(row: number, col: number): void {
    this.hoverRow = row
    this.hoverCol = col
    this.applyState()
  }

  /** Show `state` when it is this table's, the idle look otherwise. */
  setState(state: TableUiState | null): void {
    const mine = state && state.tablePos === this.pos ? state : IDLE
    this.state = mine
    this.dom.classList.toggle('is-active', mine !== IDLE)
    this.dom.classList.toggle('is-all-selected', mine.all)
    this.applyState()
  }

  private applyState(): void {
    const s = this.state
    const active = s !== IDLE
    const paint = (handles: HTMLButtonElement[], selected: number[], current: number, hover: number) => {
      handles.forEach((h, i) => {
        const isSelected = selected.includes(i)
        const isActive = !isSelected && active && current === i
        h.classList.toggle('is-selected', isSelected)
        h.classList.toggle('is-active', isActive)
        h.classList.toggle('is-hover', !isSelected && !isActive && hover === i)
      })
    }
    paint(this.rowHandles, s.rows, s.row, this.hoverRow)
    paint(this.colHandles, s.cols, s.col, this.hoverCol)
  }
}

/**
 * Keeps every table's controls on the selection, and the caret's cell in
 * view inside a table that scrolls sideways. `onState` hears where the
 * selection is (for the toolbar's Table button), null outside a table or
 * while the editor is read only.
 */
export function createTableControlsPlugin(onState: (state: TableUiState | null, editorState: EditorState) => void): Plugin {
  return new Plugin({
    key: new PluginKey('tableControls'),
    view(view) {
      let editable = view.editable
      const sync = (docChanged: boolean) => {
        editable = view.editable
        const ui = editable ? tableUiState(view.state) : null
        for (const nodeView of registryOf(view)) nodeView.setState(ui)
        onState(ui, view.state)
        if (docChanged && ui && typeof requestAnimationFrame !== 'undefined') {
          requestAnimationFrame(() => keepCaretInScroller(view))
        }
      }
      sync(false)
      return {
        update(view, prev) {
          const changed = prev.doc !== view.state.doc
          if (changed || !prev.selection.eq(view.state.selection) || editable !== view.editable) sync(changed)
        },
        destroy() {
          onState(null, view.state)
        },
      }
    },
  })
}

/** Scroll the caret's table sideways so the caret stays visible as it types. */
function keepCaretInScroller(view: EditorView): void {
  const ui = tableUiState(view.state)
  if (!ui) return
  const nodeView = Array.from(registryOf(view)).find((v) => v.pos === ui.tablePos)
  if (!nodeView) return
  let coords: { left: number; right: number }
  try {
    coords = view.coordsAtPos(view.state.selection.head)
  } catch {
    return
  }
  const scroller = nodeView.scroll
  const box = scroller.getBoundingClientRect()
  if (coords.left < box.left + 28) scroller.scrollLeft -= box.left + 28 - coords.left
  else if (coords.right > box.right - 28) scroller.scrollLeft += coords.right - (box.right - 28)
}

/** Lay every table of `view` out again (fonts loaded, the editor resized). */
export function relayoutTables(view: EditorView): void {
  for (const nodeView of registryOf(view)) nodeView.scheduleLayout()
}
