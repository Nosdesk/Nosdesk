/**
 * Column layout composable for the tickets table.
 *
 * Owns the per-view persistence + interaction logic for two
 * pieces of user state: column ORDER (which columns show, in
 * what order) and column WIDTH (px override per column id).
 *
 * Persistence:
 *   - localStorage key `tickets-columns:<viewId>` for the order
 *     array (read by the view's visibleColumnIds computed).
 *   - localStorage key `tickets-column-widths:<viewId>` for the
 *     `{ colId: widthPx }` map.
 *
 * Drag interactions:
 *   - `beginResize(colId, e)` starts a pointer-driven resize.
 *     Uses pointer capture so the drag survives the user's
 *     pointer leaving the original handle. Min/max from the
 *     column registry. Width persists on pointerup.
 *   - `dragSourceId` / `dragTargetId` track the drag-and-drop
 *     reorder state for the header row. The view binds the
 *     dragstart / dragover / drop handlers; this composable
 *     just owns the state and emits the new order on drop.
 *
 * The composable is renderless — caller renders the headers
 * + cells using the values + handlers it returns.
 */
import { ref } from 'vue'
import {
  TICKET_COLUMNS,
  type ColumnId,
  type ListColumn,
} from '@nosdesk/core/sync/views/ticketColumns'
import { useDragGesture } from '@/composables/useDragGesture'
import { useColumnReorder } from '@/composables/useColumnReorder'

const COLUMN_WIDTHS_PREFIX = 'tickets-column-widths:'

function widthsStorageKey(viewId: string): string {
  return `${COLUMN_WIDTHS_PREFIX}${viewId}`
}

export function useColumnLayout(
  getViewId: () => string,
  onOrderChange: (next: ColumnId[]) => void,
  getCurrentOrder: () => ColumnId[],
) {
  // ---------------------------------------------------------------
  // Width override map. Loaded lazily per view; the view calls
  // `loadFor(viewId)` whenever the active view changes so that
  // each view carries its own column widths without bleeding.
  // ---------------------------------------------------------------
  const widthOverrides = ref<Map<ColumnId, number>>(new Map())

  /**
   * Hydrate widths for a view. Precedence:
   *   1. localStorage override for the view (user dragged a handle
   *      this session or earlier).
   *   2. The view's canonical widths (from the saved view's
   *      `shape.columns` mapping, passed by the caller).
   *   3. Empty — defaults from the registry kick in via widthFor().
   */
  function loadFor(viewId: string, viewWidths?: Map<ColumnId, number>): void {
    const local = readWidths(viewId)
    if (local.size > 0) {
      widthOverrides.value = local
      return
    }
    widthOverrides.value = viewWidths ? new Map(viewWidths) : new Map()
  }

  function readWidths(viewId: string): Map<ColumnId, number> {
    if (typeof localStorage === 'undefined') return new Map()
    const raw = localStorage.getItem(widthsStorageKey(viewId))
    if (!raw) return new Map()
    try {
      const parsed = JSON.parse(raw)
      if (!parsed || typeof parsed !== 'object') return new Map()
      const out = new Map<ColumnId, number>()
      for (const [k, v] of Object.entries(parsed)) {
        if (typeof v === 'number' && Number.isFinite(v) && v > 0) {
          out.set(k as ColumnId, v)
        }
      }
      return out
    } catch {
      return new Map()
    }
  }

  function persistWidths(viewId: string): void {
    if (typeof localStorage === 'undefined') return
    const obj: Record<string, number> = {}
    for (const [k, v] of widthOverrides.value) obj[k] = v
    if (Object.keys(obj).length === 0) {
      localStorage.removeItem(widthsStorageKey(viewId))
    } else {
      localStorage.setItem(widthsStorageKey(viewId), JSON.stringify(obj))
    }
  }

  /** Effective render width — override if set, else the column's
   * registry default. Flex columns ignore this and use `auto`. */
  function widthFor(col: ListColumn): number {
    return widthOverrides.value.get(col.id) ?? col.defaultWidthPx
  }

  // ---------------------------------------------------------------
  // Resize. LIVE strategy: write `widthOverrides` directly on
  // each rAF tick so the table reflows in step with the cursor
  // (the conventional spreadsheet feel). The shared
  // `useDragGesture` rAF-coalesces the writes so we get at most
  // one per frame regardless of pointer event frequency.
  //
  // Double-click on the handle auto-fits the column to its
  // widest content. Detected here (not via @dblclick on the
  // template) because `useDragGesture` calls preventDefault on
  // pointerdown, which suppresses the synthetic click /
  // dblclick chain — so we count successive pointerdowns
  // within a short window instead.
  // ---------------------------------------------------------------
  const resizingId = ref<ColumnId | null>(null)
  const drag = useDragGesture()

  const DOUBLE_CLICK_MS = 350
  let lastClickAt = 0
  let lastClickColId: ColumnId | null = null

  function beginResize(colId: ColumnId, event: PointerEvent): void {
    const col = TICKET_COLUMNS.find((c) => c.id === colId)
    if (!col) return
    event.stopPropagation()

    const now = performance.now()
    if (lastClickColId === colId && now - lastClickAt < DOUBLE_CLICK_MS) {
      lastClickAt = 0
      lastClickColId = null
      const handle = event.currentTarget as HTMLElement | null
      autoFitColumn(colId, handle)
      return
    }
    lastClickAt = now
    lastClickColId = colId

    const startWidth = widthFor(col)
    resizingId.value = colId

    // Bare write helper: mutates the Map and reassigns so Vue
    // tracks the change. Used by both onUpdate (live) and the
    // pointerup commit (final). Persisted only on commit so the
    // localStorage write doesn't run 60×/second.
    const writeWidth = (px: number): void => {
      const map = new Map(widthOverrides.value)
      map.set(colId, Math.round(px))
      widthOverrides.value = map
    }

    drag.begin(event, {
      axis: 'x',
      startValue: startWidth,
      clamp: (raw) => Math.min(col.maxWidthPx, Math.max(col.minWidthPx, raw)),
      onUpdate: writeWidth,
      onCommit: (finalWidth) => {
        writeWidth(finalWidth)
        resizingId.value = null
        persistWidths(getViewId())
      },
    })
  }

  /** Fit a column to its widest visible content.
   *
   *  Measurement strategy: the table is rendered with
   *  `table-layout: fixed` and explicit per-cell widths, which
   *  means a cell's `scrollWidth` reports the *constrained*
   *  width — not the natural content width. Reading it directly
   *  gives the column's current width back, which is useless.
   *
   *  Workaround: temporarily switch the table to `table-layout:
   *  auto` and clear this column's per-cell width styles. With
   *  no constraint the browser computes each cell's natural
   *  content width on the next layout pass. We force that pass
   *  by reading `offsetWidth`, capture the values, then
   *  restore. Because the mutate → measure → restore happens
   *  inside a single synchronous task, the browser doesn't
   *  paint the intermediate state — the user sees one
   *  transition from old width to new width.
   *
   *  Result is clamped to the column's registry min/max bounds
   *  and persisted like a manual resize. */
  function autoFitColumn(colId: ColumnId, handle: HTMLElement | null): void {
    const col = TICKET_COLUMNS.find((c) => c.id === colId)
    if (!col) return

    // Scope to the same table the handle lives in so multiple
    // tables on a page (split view, modals) don't pollute each
    // other.
    const container = handle?.closest('.tickets-table-container') as HTMLElement | null
    if (!container) return
    const table = container.querySelector<HTMLTableElement>('table')
    if (!table) return
    const cells = container.querySelectorAll<HTMLElement>(`.col-${colId}`)
    if (cells.length === 0) return

    // Snapshot every style we're about to mutate so we can
    // restore the exact prior state — the originals come from
    // Vue's `:style` binding and we don't want to surprise the
    // next render.
    const savedTableLayout = table.style.tableLayout
    const savedCellStyles = Array.from(cells).map((cell) => ({
      el: cell,
      width: cell.style.width,
      minWidth: cell.style.minWidth,
      maxWidth: cell.style.maxWidth,
    }))

    // Mutate: drop the layout constraint and clear this column's
    // per-cell widths so the browser sizes them to content.
    table.style.tableLayout = 'auto'
    cells.forEach((cell) => {
      cell.style.width = 'auto'
      cell.style.minWidth = '0'
      cell.style.maxWidth = 'none'
    })

    // Measure: reading offsetWidth on each cell forces the
    // browser to lay out with the new (unconstrained) styles.
    let naturalMax = col.minWidthPx
    cells.forEach((cell) => {
      const w = cell.offsetWidth
      if (w > naturalMax) naturalMax = w
    })

    // Restore — synchronous within the same JS task, so the
    // intermediate auto-layout never paints.
    table.style.tableLayout = savedTableLayout
    savedCellStyles.forEach((s) => {
      s.el.style.width = s.width
      s.el.style.minWidth = s.minWidth
      s.el.style.maxWidth = s.maxWidth
    })

    // Small breathing-room margin so content isn't flush
    // against the column edge, then clamp to registry bounds.
    const target = Math.min(
      col.maxWidthPx,
      Math.max(col.minWidthPx, naturalMax + 8),
    )

    const map = new Map(widthOverrides.value)
    map.set(colId, Math.round(target))
    widthOverrides.value = map
    persistWidths(getViewId())
  }

  // ---------------------------------------------------------------
  // Reorder via HTML5 drag-and-drop. Delegated to the shared
  // `useColumnReorder` composable so the gesture state + drop
  // reducer match the behaviour DataTable consumers get.
  //
  // The title column doesn't participate — it's pinned first
  // because of its flex behaviour and would be confusing if a
  // user could push it mid-row only to have it bounce back.
  //
  // Resize and reorder both initiate from a mousedown inside the
  // same draggable <th>. The resize handle's pointerdown fires
  // synchronously and sets `resizingId` before the dragstart
  // bubbles up — when that flag is set the drag is cancelled so
  // the pointer-driven resize loop owns the gesture.
  // ---------------------------------------------------------------
  const reorder = useColumnReorder({
    isReorderable: (id) => id !== 'title',
    getCurrentOrder: () => getCurrentOrder(),
    onOrderChange: (next) => onOrderChange(next as ColumnId[]),
  })

  const { dragSourceId, dragTargetId, isReorderable, onDragOver, onDragLeave, onDragEnd } =
    reorder

  function onDragStart(colId: ColumnId, event: DragEvent): void {
    if (resizingId.value !== null) {
      event.preventDefault()
      return
    }
    reorder.onDragStart(colId, event)
  }

  function onDrop(colId: ColumnId, event: DragEvent): void {
    reorder.onDrop(colId, event)
  }

  function clearWidths(): void {
    widthOverrides.value = new Map()
    persistWidths(getViewId())
  }

  return {
    widthOverrides,
    widthFor,
    loadFor,
    persistWidths: () => persistWidths(getViewId()),
    clearWidths,
    // Resize
    resizingId,
    beginResize,
    autoFitColumn,
    // Reorder
    dragSourceId,
    dragTargetId,
    isReorderable,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragEnd,
  }
}
