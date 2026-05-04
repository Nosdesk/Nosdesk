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
} from '@/sync/views/ticketColumns'

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
  // Resize. The view renders a 4px hit-area inside each <th>;
  // pointerdown on the handle calls beginResize() to wire up the
  // pointer-capture loop.
  // ---------------------------------------------------------------
  const resizingId = ref<ColumnId | null>(null)

  function beginResize(colId: ColumnId, event: PointerEvent): void {
    const col = TICKET_COLUMNS.find((c) => c.id === colId)
    if (!col) return
    event.preventDefault()
    event.stopPropagation()
    const startX = event.clientX
    const startWidth = widthFor(col)
    const target = event.currentTarget as HTMLElement
    target.setPointerCapture(event.pointerId)
    resizingId.value = colId

    const onMove = (e: PointerEvent) => {
      const delta = e.clientX - startX
      let next = startWidth + delta
      if (next < col.minWidthPx) next = col.minWidthPx
      if (next > col.maxWidthPx) next = col.maxWidthPx
      // Mutate the Map in place + reassign so Vue tracks the change.
      const map = new Map(widthOverrides.value)
      map.set(colId, Math.round(next))
      widthOverrides.value = map
    }

    const onUp = (e: PointerEvent) => {
      target.releasePointerCapture?.(e.pointerId)
      target.removeEventListener('pointermove', onMove)
      target.removeEventListener('pointerup', onUp)
      target.removeEventListener('pointercancel', onUp)
      resizingId.value = null
      persistWidths(getViewId())
    }

    target.addEventListener('pointermove', onMove)
    target.addEventListener('pointerup', onUp)
    target.addEventListener('pointercancel', onUp)
  }

  // ---------------------------------------------------------------
  // Reorder via HTML5 drag-and-drop. Headers set draggable=true;
  // these handlers track the source + target and emit the new
  // order on drop. Title doesn't participate — it's pinned first
  // because of its flex behaviour and would be confusing if a
  // user could push it mid-row only to have it bounce back.
  // ---------------------------------------------------------------
  const dragSourceId = ref<ColumnId | null>(null)
  const dragTargetId = ref<ColumnId | null>(null)

  function isReorderable(colId: ColumnId): boolean {
    return colId !== 'title'
  }

  function onDragStart(colId: ColumnId, event: DragEvent): void {
    if (!isReorderable(colId)) {
      event.preventDefault()
      return
    }
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move'
      event.dataTransfer.setData('text/plain', colId)
    }
    dragSourceId.value = colId
  }

  function onDragOver(colId: ColumnId, event: DragEvent): void {
    if (!dragSourceId.value || !isReorderable(colId)) return
    if (colId === dragSourceId.value) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    dragTargetId.value = colId
  }

  function onDragLeave(colId: ColumnId): void {
    if (dragTargetId.value === colId) dragTargetId.value = null
  }

  function onDrop(colId: ColumnId, event: DragEvent): void {
    event.preventDefault()
    const source = dragSourceId.value
    dragSourceId.value = null
    dragTargetId.value = null
    if (!source || source === colId || !isReorderable(colId)) return

    const order = [...getCurrentOrder()]
    const fromIdx = order.indexOf(source)
    const toIdx = order.indexOf(colId)
    if (fromIdx < 0 || toIdx < 0) return
    order.splice(fromIdx, 1)
    order.splice(toIdx, 0, source)
    onOrderChange(order)
  }

  function onDragEnd(): void {
    dragSourceId.value = null
    dragTargetId.value = null
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
