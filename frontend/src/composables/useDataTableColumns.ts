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

/** Minimal column shape this composable reads. Consumers'
 *  richer column types extend / overlap this. */
export interface DataTableColumnLike {
  /** Stable identifier. Used for persistence and drag tracking. */
  field: string
  /** Header label fallback (for the picker). */
  label: string
  /** When set, default-hide the column until the user opts in. */
  defaultHidden?: boolean
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
  /** Replace the entire order + hidden state at once. Used by
   *  saved-view apply to restore a layout from the view's shape. */
  applyLayout: (order: string[], hidden: string[]) => void
  /** Snapshot the current order + hidden set for saved-view
   *  capture. */
  captureLayout: () => { order: string[]; hidden: string[] }
  /** Reset to the registry's default order + visibility. Clears
   *  the localStorage rows for the current view. */
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
}

function orderStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-columns-order:${viewId}`
}

function hiddenStorageKey(namespace: string, viewId: string): string {
  return `${namespace}-columns-hidden:${viewId}`
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

  // Initialise from storage or registry defaults. Both refs are
  // string arrays / sets keyed by `column.field`, decoupled from
  // the C type so reactive identity is straightforward.
  const order = ref<string[]>(initialOrder())
  const hidden = ref<Set<string>>(initialHidden())

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

  const ordered = computed<C[]>(() =>
    order.value.map((id) => byField.value.get(id)).filter((c): c is C => !!c),
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
  }

  function toggleVisible(field: string): void {
    if (pinned.has(field)) return
    const next = new Set(hidden.value)
    if (next.has(field)) next.delete(field)
    else next.add(field)
    hidden.value = next
    persist()
  }

  function applyLayout(nextOrder: string[], nextHidden: string[]): void {
    order.value = reconcileOrder(nextOrder)
    hidden.value = new Set(nextHidden)
    persist()
  }

  function captureLayout(): { order: string[]; hidden: string[] } {
    return {
      order: [...order.value],
      hidden: [...hidden.value],
    }
  }

  function reset(): void {
    order.value = columns.value.map((c) => c.field)
    hidden.value = new Set(
      columns.value.filter((c) => c.defaultHidden).map((c) => c.field),
    )
    if (typeof localStorage !== 'undefined') {
      localStorage.removeItem(orderStorageKey(storageNamespace, getViewId()))
      localStorage.removeItem(hiddenStorageKey(storageNamespace, getViewId()))
    }
  }

  // ---- Drag-reorder ---------------------------------------------
  const dragSourceId = ref<string | null>(null)
  const dragTargetId = ref<string | null>(null)

  function isReorderable(field: string): boolean {
    return !pinned.has(field)
  }

  function onDragStart(field: string, event: DragEvent): void {
    if (!isReorderable(field)) {
      event.preventDefault()
      return
    }
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move'
      event.dataTransfer.setData('text/plain', field)
    }
    dragSourceId.value = field
  }

  function onDragOver(field: string, event: DragEvent): void {
    if (!dragSourceId.value || !isReorderable(field)) return
    if (field === dragSourceId.value) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    dragTargetId.value = field
  }

  function onDragLeave(field: string): void {
    if (dragTargetId.value === field) dragTargetId.value = null
  }

  function onDrop(field: string, event: DragEvent): void {
    event.preventDefault()
    const source = dragSourceId.value
    dragSourceId.value = null
    dragTargetId.value = null
    if (!source || source === field || !isReorderable(field)) return

    const next = [...order.value]
    const fromIdx = next.indexOf(source)
    const toIdx = next.indexOf(field)
    if (fromIdx < 0 || toIdx < 0) return
    next.splice(fromIdx, 1)
    next.splice(toIdx, 0, source)
    order.value = next
    persist()
  }

  function onDragEnd(): void {
    dragSourceId.value = null
    dragTargetId.value = null
  }

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
  }
}
