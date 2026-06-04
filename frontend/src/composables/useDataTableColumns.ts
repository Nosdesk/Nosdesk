/**
 * Column order + visibility for the shared DataTable (assets,
 * users, and any future grid-rendered list view). The tickets
 * table has its own equivalent in `useColumnLayout` because its
 * <table>-with-table-layout:fixed rendering takes a different
 * width-override approach; this composable is the CSS-grid /
 * DataTable counterpart.
 *
 * Owns two pieces of per-view state:
 * - **order**: an array of column ids in render order. Reorder
 *   drops mutate the array in place and persist.
 * - **hidden**: a Set of column ids explicitly hidden by the
 *   user (the column picker checkbox cleared). Columns not
 *   listed default to visible; switching off a default-visible
 *   column adds it to `hidden`, switching it back on removes
 *   it.
 *
 * Persistence:
 *   {namespace}-columns-order:{viewId}    -> JSON string[]
 *   {namespace}-columns-hidden:{viewId}   -> JSON string[]
 *
 * Reorder gestures use the HTML5 drag-and-drop API like the
 * tickets equivalent: the consumer binds `dragstart` /
 * `dragover` / `drop` / `dragend` from the composable on each
 * header; the composable owns the source / target refs and
 * commits a new order on drop. Pinned columns (eg. a name
 * column you don't want users to push off-screen) can be
 * excluded via `pinnedIds`.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import { useDragGesture } from '@/composables/useDragGesture'
import { useColumnReorder } from '@/composables/useColumnReorder'

/** Default resize bounds when a column doesn't declare its own.
 *  Mirrors typical helpdesk-list ergonomics: too narrow and the
 *  header label clips; too wide and a single column eats the row. */
const DEFAULT_MIN_WIDTH_PX = 60
const DEFAULT_MAX_WIDTH_PX = 800

/** Minimal column shape this composable reads. Consumers'
 *  richer column types extend / overlap this. */
export interface DataTableColumnLike {
  /** Stable identifier. Used for persistence and drag tracking. */
  field: string
  /** Header label fallback (for the picker). */
  label: string
  /** Default grid-template-columns slot for this column (string
   *  passed to CSS, eg. '1fr', 'minmax(140px,auto)', '120px').
   *  Overridden by the resize state when the user drags a
   *  header edge; the override is always a `${px}px` string. */
  width?: string
  /** When set, default-hide the column until the user opts in. */
  defaultHidden?: boolean
  /** Resize lower bound in pixels. Falls back to a sensible
   *  default if absent. */
  minWidthPx?: number
  /** Resize upper bound in pixels. Falls back to a sensible
   *  default if absent. */
  maxWidthPx?: number
}

export interface UseDataTableColumnsOptions<C extends DataTableColumnLike> {
  /** Reactive column registry. Order in the array is the default
   *  order; the persisted order overrides this when present. */
  columns: ComputedRef<readonly C[]> | Ref<readonly C[]>
  /** Per-dataset prefix so 'assets', 'users', etc. don't share
   *  the same localStorage key. */
  storageNamespace: string
  /** Per-view scope id getter. Each view (default + each saved
   *  view) carries its own order + hidden state. */
  getViewId: () => string
  /** Column ids that cannot be reordered or hidden. Typically
   *  the primary name / title column so users can't accidentally
   *  push it off-screen. */
  pinnedIds?: readonly string[]
}

export interface UseDataTableColumns<C extends DataTableColumnLike> {
  /** Columns in current render order with hidden ones filtered out. */
  visible: ComputedRef<C[]>
  /** All columns in current order, regardless of visibility (for
   *  the picker checklist). */
  ordered: ComputedRef<C[]>
  isHidden: (field: string) => boolean
  isPinned: (field: string) => boolean
  toggleVisible: (field: string) => void
  /** Replace the entire order + hidden + widths state at once.
   *  Used by saved-view apply to restore a layout from the
   *  view's shape. Widths are field -> px. */
  applyLayout: (
    order: string[],
    hidden: string[],
    widths?: Record<string, number>,
  ) => void
  /** Snapshot the current layout for saved-view capture. */
  captureLayout: () => {
    order: string[]
    hidden: string[]
    widths: Record<string, number>
  }
  /** Reset to the registry's default order, visibility, and
   *  widths. Clears the localStorage rows for the current view. */
  reset: () => void

  // Drag-reorder state + handlers
  dragSourceId: Ref<string | null>
  dragTargetId: Ref<string | null>
  isReorderable: (field: string) => boolean
  onDragStart: (field: string, event: DragEvent) => void
  onDragOver: (field: string, event: DragEvent) => void
  onDragLeave: (field: string) => void
  onDrop: (field: string, event: DragEvent) => void
  onDragEnd: () => void
  /** Pre-shaped bundle matching DataTable's `columnReorder`
   *  prop type. Consumers pass this through in one binding
   *  instead of spelling out the eight fields inline. */
  reorderBundle: {
    sourceId: Ref<string | null>
    targetId: Ref<string | null>
    isReorderable: (field: string) => boolean
    onDragStart: (field: string, event: DragEvent) => void
    onDragOver: (field: string, event: DragEvent) => void
    onDragLeave: (field: string) => void
    onDrop: (field: string, event: DragEvent) => void
    onDragEnd: () => void
  }

  // Column resize state + handlers
  resizingId: Ref<string | null>
  /** Begin a pointer-driven resize. `startWidthPx` is the
   *  column's current rendered width — the DataTable's handle
   *  measures the header's offsetWidth on pointerdown and
   *  passes it in (the composable doesn't have DOM access
   *  itself). */
  beginResize: (field: string, event: PointerEvent, startWidthPx: number) => void
  /** Pre-shaped bundle matching DataTable's `columnResize` prop. */
  resizeBundle: {
    resizingId: Ref<string | null>
    onResizeStart: (field: string, event: PointerEvent, startWidthPx: number) => void
  }
}

function orderStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-columns-order:${viewId}`
}

function hiddenStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-columns-hidden:${viewId}`
}

function widthsStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-columns-widths:${viewId}`
}

function loadWidths(key: string): Map<string, number> | null {
  if (typeof localStorage === 'undefined') return null
  const raw = localStorage.getItem(key)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object') return null
    const out = new Map<string, number>()
    for (const [k, v] of Object.entries(parsed)) {
      if (typeof v === 'number' && Number.isFinite(v) && v > 0) {
        out.set(k, v)
      }
    }
    return out
  } catch {
    return null
  }
}

function persistWidths(key: string, value: Map<string, number>): void {
  if (typeof localStorage === 'undefined') return
  if (value.size === 0) {
    localStorage.removeItem(key)
    return
  }
  const obj: Record<string, number> = {}
  for (const [k, v] of value) obj[k] = v
  localStorage.setItem(key, JSON.stringify(obj))
}

function loadStringArray(key: string): string[] | null {
  if (typeof localStorage === 'undefined') return null
  const raw = localStorage.getItem(key)
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return null
    return parsed.filter((x): x is string => typeof x === 'string')
  } catch {
    return null
  }
}

function persistStringArray(key: string, value: string[]): void {
  if (typeof localStorage === 'undefined') return
  if (value.length === 0) localStorage.removeItem(key)
  else localStorage.setItem(key, JSON.stringify(value))
}

export function useDataTableColumns<C extends DataTableColumnLike>(
  options: UseDataTableColumnsOptions<C>,
): UseDataTableColumns<C> {
  const { columns, storageNamespace, getViewId, pinnedIds = [] } = options
  const pinned = new Set(pinnedIds)

  // Initialise from storage or registry defaults. All refs are
  // keyed by `column.field`, decoupled from the C type so
  // reactive identity is straightforward.
  const order = ref<string[]>(initialOrder())
  const hidden = ref<Set<string>>(initialHidden())
  const widthOverrides = ref<Map<string, number>>(initialWidths())
  const resizingId = ref<string | null>(null)

  function initialOrder(): string[] {
    const stored = loadStringArray(orderStorageKey(storageNamespace, getViewId()))
    if (stored && stored.length > 0) return reconcileOrder(stored)
    return columns.value.map((c) => c.field)
  }

  function initialHidden(): Set<string> {
    const stored = loadStringArray(hiddenStorageKey(storageNamespace, getViewId()))
    if (stored) return new Set(stored)
    // First-time defaults from the registry.
    return new Set(columns.value.filter((c) => c.defaultHidden).map((c) => c.field))
  }

  function initialWidths(): Map<string, number> {
    return loadWidths(widthsStorageKey(storageNamespace, getViewId())) ?? new Map()
  }

  /** Drop ids that no longer exist in the registry; append new
   *  ids the user hasn't seen yet to the tail so adding a column
   *  to the registry surfaces it without wiping the user's
   *  customisation. */
  function reconcileOrder(stored: string[]): string[] {
    const registryIds = new Set(columns.value.map((c) => c.field))
    const cleaned = stored.filter((id) => registryIds.has(id))
    for (const c of columns.value) {
      if (!cleaned.includes(c.field)) cleaned.push(c.field)
    }
    return cleaned
  }

  // Reload state when the active view changes (saved-view
  // story). Each view (built-in or saved) carries its own
  // layout, mirroring how `useListGrouping` scopes per-view.
  watch(
    () => getViewId(),
    () => {
      order.value = initialOrder()
      hidden.value = initialHidden()
      widthOverrides.value = initialWidths()
    },
  )

  // Keep the order array in sync if the registry itself changes
  // (eg. a column added or removed at runtime). Reconcile but
  // do not persist, because reconciliation isn't a user-driven
  // change.
  watch(
    columns,
    () => {
      order.value = reconcileOrder(order.value)
    },
  )

  const byField = computed<Map<string, C>>(() => {
    const m = new Map<string, C>()
    for (const c of columns.value) m.set(c.field, c)
    return m
  })

  /** Apply the user's px width override to a column copy. Leaves
   *  the registry default `width` string in place when no
   *  override exists, so flex (`'1fr'`) and `minmax(...)` slots
   *  keep their CSS-grid semantics until the user manually
   *  resizes.
   *
   *  A flexible (`fr`) column is the one that absorbs slack so the
   *  grid fills its container; never pin it to a stored pixel width
   *  (which would leave dead space on wide displays). This also
   *  self-heals a stale override left from before the column was
   *  made non-resizable. */
  function withEffectiveWidth(col: C): C {
    if (typeof col.width === 'string' && col.width.includes('fr')) return col
    const override = widthOverrides.value.get(col.field)
    if (override == null) return col
    return { ...col, width: `${override}px` }
  }

  const ordered = computed<C[]>(() =>
    order.value
      .map((id) => byField.value.get(id))
      .filter((c): c is C => !!c)
      .map(withEffectiveWidth),
  )

  const visible = computed<C[]>(() =>
    ordered.value.filter((c) => !hidden.value.has(c.field)),
  )

  function isHidden(field: string): boolean {
    return hidden.value.has(field)
  }

  function isPinned(field: string): boolean {
    return pinned.has(field)
  }

  function persist(): void {
    persistStringArray(
      orderStorageKey(storageNamespace, getViewId()),
      order.value,
    )
    persistStringArray(
      hiddenStorageKey(storageNamespace, getViewId()),
      [...hidden.value],
    )
    persistWidths(
      widthsStorageKey(storageNamespace, getViewId()),
      widthOverrides.value,
    )
  }

  function toggleVisible(field: string): void {
    if (pinned.has(field)) return
    const next = new Set(hidden.value)
    if (next.has(field)) next.delete(field)
    else next.add(field)
    hidden.value = next
    persist()
  }

  function applyLayout(
    nextOrder: string[],
    nextHidden: string[],
    nextWidths?: Record<string, number>,
  ): void {
    order.value = reconcileOrder(nextOrder)
    hidden.value = new Set(nextHidden)
    if (nextWidths) {
      const map = new Map<string, number>()
      for (const [k, v] of Object.entries(nextWidths)) {
        if (typeof v === 'number' && Number.isFinite(v) && v > 0) {
          map.set(k, v)
        }
      }
      widthOverrides.value = map
    } else {
      widthOverrides.value = new Map()
    }
    persist()
  }

  function captureLayout(): {
    order: string[]
    hidden: string[]
    widths: Record<string, number>
  } {
    const widths: Record<string, number> = {}
    for (const [k, v] of widthOverrides.value) widths[k] = v
    return {
      order: [...order.value],
      hidden: [...hidden.value],
      widths,
    }
  }

  function reset(): void {
    order.value = columns.value.map((c) => c.field)
    hidden.value = new Set(
      columns.value.filter((c) => c.defaultHidden).map((c) => c.field),
    )
    widthOverrides.value = new Map()
    if (typeof localStorage !== 'undefined') {
      localStorage.removeItem(orderStorageKey(storageNamespace, getViewId()))
      localStorage.removeItem(hiddenStorageKey(storageNamespace, getViewId()))
      localStorage.removeItem(widthsStorageKey(storageNamespace, getViewId()))
    }
  }

  // ---- Column resize -------------------------------------------
  // CSS-grid layout means the column's rendered width comes
  // from `grid-template-columns`, not from any `width` style on
  // the cell. The composable maintains a px override per field
  // and bakes it into the `visible` / `ordered` columns above;
  // DataTable's grid template recomputes whenever the override
  // changes. Pointer-driven resize uses the shared
  // `useDragGesture` composable for rAF-coalesced live updates.
  const resizeDrag = useDragGesture()

  function beginResize(
    field: string,
    event: PointerEvent,
    startWidthPx: number,
  ): void {
    if (!Number.isFinite(startWidthPx) || startWidthPx <= 0) return
    const col = columns.value.find((c) => c.field === field)
    if (!col) return
    event.stopPropagation()

    const minPx = col.minWidthPx ?? DEFAULT_MIN_WIDTH_PX
    const maxPx = col.maxWidthPx ?? DEFAULT_MAX_WIDTH_PX
    resizingId.value = field

    const writeWidth = (px: number): void => {
      const map = new Map(widthOverrides.value)
      map.set(field, Math.round(px))
      widthOverrides.value = map
    }

    resizeDrag.begin(event, {
      axis: 'x',
      startValue: startWidthPx,
      clamp: (raw) => Math.min(maxPx, Math.max(minPx, raw)),
      onUpdate: writeWidth,
      onCommit: (finalWidth) => {
        writeWidth(finalWidth)
        resizingId.value = null
        persist()
      },
    })
  }

  // ---- Drag-reorder ---------------------------------------------
  // Gesture state + drop reducer come from the shared
  // `useColumnReorder` composable; this composable just wires
  // the persisted `order` ref through it. Pinned columns are
  // non-reorderable.
  const reorder = useColumnReorder({
    isReorderable: (id) => !pinned.has(id),
    getCurrentOrder: () => order.value,
    onOrderChange: (next) => {
      order.value = next
      persist()
    },
  })

  const {
    dragSourceId,
    dragTargetId,
    isReorderable,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragEnd,
  } = reorder

  return {
    visible,
    ordered,
    isHidden,
    isPinned,
    toggleVisible,
    applyLayout,
    captureLayout,
    reset,
    dragSourceId,
    dragTargetId,
    isReorderable,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragEnd,
    reorderBundle: {
      sourceId: dragSourceId,
      targetId: dragTargetId,
      isReorderable,
      onDragStart,
      onDragOver,
      onDragLeave,
      onDrop,
      onDragEnd,
    },
    resizingId,
    beginResize,
    resizeBundle: {
      resizingId,
      onResizeStart: beginResize,
    },
  }
}
