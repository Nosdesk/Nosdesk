/**
 * Pointer-event sortable list. The engine is a pure state machine
 * emitting drop intent; the view decides how to render it.
 *
 * HTML5 DnD is avoided across the app (touch support, dragleave-on-
 * children, crippled drag image). This composable uses Pointer
 * Events, same pattern as `useKanbanDragDrop`.
 *
 * Rendering contract:
 *   - The view renders items with `data-sortable-index="${n}"` on
 *     each item's root element. `n` is the ORIGINAL index into the
 *     underlying array.
 *   - The view wires `@pointerdown` on a drag handle inside each
 *     item to `handlePointerDown(index, event)`.
 *   - On successful drop the composable calls `onReorder(from, to)`.
 *   - The view DOES NOT reorder its rendered DOM during the drag.
 *     The DOM order stays equal to `visibleEntries`. The view layer
 *     uses `packGrid` + `projectedTargetIndex` (see below) to build
 *     a per-widget `transform: translate(dx, dy)` map that shifts
 *     widgets to their projected post-commit cells. CSS transitions
 *     on `transform` animate the slide. Keeping the DOM stable means
 *     pointer capture survives the drag and no FLIP machinery has to
 *     fight per-cursor-move re-renders. The transforms are exact
 *     because the grid is a fixed-unit lattice (`grid-auto-rows`), so
 *     a lattice-cell delta maps to a constant pixel offset.
 *
 * State the view consumes:
 *   - `dragState.isDragging`, `sourceIndex`, `hoverIndex`, `dropBefore` —
 *     the classic engine fields. `hoverIndex` + `dropBefore` are the
 *     intent for the next commit; the caller projects them into the
 *     post-commit list via `projectedTargetIndex`.
 *   - `dragState.renderedColumns` — snapshotted at drag-start from the
 *     grid container's computed `grid-template-columns`. Lets the
 *     view clamp the dragged widget's effective column span under
 *     the `grid-cols-1` mobile layout without a parallel code path.
 *
 * Hit-testing (unchanged from the pre-2026-06 engine):
 *   - Every sortable item's rect is frozen at drag start and reused
 *     for the duration of the drag. Hit-testing against the original
 *     layout decouples it from any visual movement and prevents the
 *     cursor from chasing the moving preview (a problem the deferred-
 *     reorder model independently sidesteps).
 *   - Rect containment is the selector. Within a target, the split
 *     axis is viewport-aware: on multi-column layouts (>= Tailwind
 *     `xl`, 1280px) the X-midline splits before/after; on single-
 *     column layouts the Y-midline splits.
 *   - Cursor in a gap between widgets: the last known target is
 *     retained so the indicator doesn't flicker off.
 *
 * Touch + iOS Safari hardening:
 *   - `lostpointercapture` is treated as cancel (iOS Safari long-
 *     press contextmenu can steal capture).
 *   - `contextmenu` is `preventDefault`'d while dragging so the iOS
 *     long-press menu doesn't appear under the finger.
 *   - Auto-scroll: when the cursor is within `EDGE_SCROLL_BAND` of
 *     the viewport top or bottom, a rAF loop scrolls the document at
 *     a rate proportional to edge proximity. Without this, mobile
 *     dashboards with more widgets than fit the viewport are
 *     unreorderable past the initial fold.
 */
import { onBeforeUnmount, onMounted, reactive, type Ref } from 'vue'

export interface PointerSortableOptions {
  enabled: Ref<boolean>
  onReorder: (fromIndex: number, toIndex: number) => void
  /** Called on pointerup with no movement or an invalid target so
   *  the caller can run the soft outline pulse on the source widget.
   *  Optional; default is a no-op. */
  onInvalidDrop?: (sourceIndex: number) => void
  /** Returns the grid container element so the engine can snapshot
   *  the rendered column count at drag-start. Without this the
   *  indicator falls back to assuming a single column. */
  getGridEl?: () => HTMLElement | null
  clickThreshold?: number
  touchHoldMs?: number
}

interface FrozenRect {
  index: number
  rect: DOMRect
}

export interface DragState {
  isDragging: boolean
  /** Original index of the dragged item, or -1 when idle. */
  sourceIndex: number
  /** Original index of the widget the cursor is currently over. */
  hoverIndex: number
  /** True = insert before `hoverIndex`, false = insert after. */
  dropBefore: boolean
  /** Snapshot of the grid container's rendered column count at
   *  drag-start. Lets the view clamp the dragged widget's effective
   *  span. At xl+ this is typically 3; below xl the grid collapses
   *  to 1 and the math degrades cleanly to a single-column list. */
  renderedColumns: number
}

const ITEM_ATTR = 'data-sortable-index'

/** Cursor must be within this many pixels of the viewport top or
 *  bottom for the auto-scroll rAF loop to fire. */
const EDGE_SCROLL_BAND = 60
/** Maximum scroll velocity (px per frame) at the very edge. Falls
 *  off linearly to 0 at the band boundary. */
const EDGE_SCROLL_MAX = 4
/** Tailwind's `xl` breakpoint. Below this the dashboard grid
 *  collapses to a single column. Cached once per drag so the axis
 *  doesn't flip mid-gesture on a resize. */
const MULTI_COLUMN_MIN_WIDTH = 1280

/** Count rendered grid columns from `grid-template-columns`. Falls
 *  back to 1 when the grid is single-column (mobile) or unavailable. */
export function snapshotGridColumnCount(el: HTMLElement | null): number {
  if (!el) return 1
  const tracks = getComputedStyle(el).gridTemplateColumns.trim()
  if (!tracks || tracks === 'none') return 1
  return tracks.split(/\s+/).filter(Boolean).length
}

export function usePointerSortable(options: PointerSortableOptions) {
  const { enabled, onReorder, onInvalidDrop, getGridEl } = options
  const clickThreshold = options.clickThreshold ?? 5
  const touchHoldMs = options.touchHoldMs ?? 400

  const dragState = reactive<DragState>({
    isDragging: false,
    sourceIndex: -1,
    hoverIndex: -1,
    dropBefore: true,
    renderedColumns: 1,
  })

  let frozenRects: FrozenRect[] = []
  let multiColumnLayout = false
  let startPos = { x: 0, y: 0 }
  let lastPointer = { x: 0, y: 0 }
  let pendingIndex = -1
  let pendingPointerId = -1
  let pendingTarget: HTMLElement | null = null
  let activePointerId = -1
  let holdTimer: ReturnType<typeof setTimeout> | null = null
  let pointerMoved = false
  let autoScrollRaf = 0

  function reset() {
    if (holdTimer) {
      clearTimeout(holdTimer)
      holdTimer = null
    }
    if (autoScrollRaf) {
      cancelAnimationFrame(autoScrollRaf)
      autoScrollRaf = 0
    }
    frozenRects = []
    pendingIndex = -1
    pendingPointerId = -1
    pendingTarget = null
    activePointerId = -1
    pointerMoved = false
    dragState.isDragging = false
    dragState.sourceIndex = -1
    dragState.hoverIndex = -1
    dragState.dropBefore = true
    dragState.renderedColumns = 1
  }

  function snapshotRenderedColumns(): number {
    return snapshotGridColumnCount(getGridEl?.() ?? null)
  }

  function freezeRects() {
    frozenRects = []
    for (const el of document.querySelectorAll<HTMLElement>(`[${ITEM_ATTR}]`)) {
      const i = Number(el.getAttribute(ITEM_ATTR))
      if (Number.isFinite(i)) {
        frozenRects.push({ index: i, rect: el.getBoundingClientRect() })
      }
    }
    multiColumnLayout = window.innerWidth >= MULTI_COLUMN_MIN_WIDTH
    dragState.renderedColumns = snapshotRenderedColumns()
  }

  /** Widget rect containing (x, y), excluding the source. `null` if
   *  the cursor is in a gap. */
  function findTarget(x: number, y: number): FrozenRect | null {
    for (const fr of frozenRects) {
      if (fr.index === dragState.sourceIndex) continue
      const r = fr.rect
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
        return fr
      }
    }
    // Trailing empty cells in a partly-filled row have no widget rect.
    // On multi-column layouts, target the rightmost widget on the
    // cursor's Y row so `applyTarget` projects an insert-after slot.
    // Without this, dropping into a row's empty trailing columns is
    // impossible (e.g. two 1x1 widgets in a 3-col row leave col 3
    // unreachable).
    if (multiColumnLayout) {
      let chosen: FrozenRect | null = null
      for (const fr of frozenRects) {
        if (fr.index === dragState.sourceIndex) continue
        if (y < fr.rect.top || y > fr.rect.bottom) continue
        if (!chosen || fr.rect.right > chosen.rect.right) chosen = fr
      }
      if (chosen) return chosen
    }
    return null
  }

  /** The bottom-most visible sibling, excluding the source. */
  function findTrailingTarget(): FrozenRect | null {
    let chosen: FrozenRect | null = null
    for (const fr of frozenRects) {
      if (fr.index === dragState.sourceIndex) continue
      if (!chosen || fr.rect.bottom > chosen.rect.bottom) chosen = fr
    }
    return chosen
  }

  function applyTarget(fr: FrozenRect, cursorX: number, cursorY: number) {
    dragState.hoverIndex = fr.index
    if (multiColumnLayout) {
      const midX = fr.rect.left + fr.rect.width / 2
      dragState.dropBefore = cursorX < midX
    } else {
      const midY = fr.rect.top + fr.rect.height / 2
      dragState.dropBefore = cursorY < midY
    }
  }

  function tickAutoScroll() {
    autoScrollRaf = 0
    if (!dragState.isDragging) return
    const y = lastPointer.y
    const viewH = window.innerHeight
    let delta = 0
    if (y < EDGE_SCROLL_BAND) {
      const distance = Math.max(0, y)
      const ratio = 1 - distance / EDGE_SCROLL_BAND
      delta = -Math.round(EDGE_SCROLL_MAX * ratio)
    } else if (y > viewH - EDGE_SCROLL_BAND) {
      const distance = Math.max(0, viewH - y)
      const ratio = 1 - distance / EDGE_SCROLL_BAND
      delta = Math.round(EDGE_SCROLL_MAX * ratio)
    }
    if (delta !== 0) window.scrollBy(0, delta)
    autoScrollRaf = requestAnimationFrame(tickAutoScroll)
  }

  function maybeScheduleAutoScroll() {
    if (!dragState.isDragging) return
    if (autoScrollRaf !== 0) return
    autoScrollRaf = requestAnimationFrame(tickAutoScroll)
  }

  function onPointerMove(e: PointerEvent) {
    // Touch/pen hold-to-drag: cancel if the finger moves before the
    // hold timer fires.
    if (pendingIndex !== -1 && holdTimer) {
      const dx = Math.abs(e.clientX - startPos.x)
      const dy = Math.abs(e.clientY - startPos.y)
      if (dx > clickThreshold || dy > clickThreshold) {
        clearTimeout(holdTimer)
        holdTimer = null
        pendingIndex = -1
      }
      return
    }

    if (!dragState.isDragging) return

    lastPointer = { x: e.clientX, y: e.clientY }
    const dx = Math.abs(e.clientX - startPos.x)
    const dy = Math.abs(e.clientY - startPos.y)
    if (dx > clickThreshold || dy > clickThreshold) pointerMoved = true

    const target = findTarget(e.clientX, e.clientY)
    if (target) {
      applyTarget(target, e.clientX, e.clientY)
    } else {
      // Cursor outside every sibling's rect. If below all of them,
      // treat as "append after the last widget" — this is how a user
      // drags an item into a new (empty) trailing row. Otherwise
      // leave the last-known hover intact so the indicator doesn't
      // flicker off in inter-row gaps.
      const trailing = findTrailingTarget()
      if (trailing && e.clientY > trailing.rect.bottom) {
        dragState.hoverIndex = trailing.index
        dragState.dropBefore = false
      }
    }

    maybeScheduleAutoScroll()
  }

  function onPointerUp() {
    if (holdTimer) {
      clearTimeout(holdTimer)
      holdTimer = null
      pendingIndex = -1
      return
    }

    if (!dragState.isDragging) {
      reset()
      return
    }

    const { sourceIndex, hoverIndex, dropBefore } = dragState
    const moved = pointerMoved
    reset()

    if (!moved || sourceIndex === -1 || hoverIndex === -1) {
      if (sourceIndex !== -1) onInvalidDrop?.(sourceIndex)
      return
    }
    if (sourceIndex === hoverIndex) {
      onInvalidDrop?.(sourceIndex)
      return
    }

    let to = dropBefore ? hoverIndex : hoverIndex + 1
    if (to > sourceIndex) to -= 1
    if (to === sourceIndex) {
      onInvalidDrop?.(sourceIndex)
      return
    }

    onReorder(sourceIndex, to)
  }

  function onLostPointerCapture(e: PointerEvent) {
    if (!dragState.isDragging) return
    if (activePointerId !== -1 && e.pointerId !== activePointerId) return
    reset()
  }

  function onContextMenu(e: Event) {
    if (dragState.isDragging) e.preventDefault()
  }

  function beginDrag(index: number, pointerId: number, target: HTMLElement | null) {
    dragState.isDragging = true
    dragState.sourceIndex = index
    freezeRects()
    const first = findTarget(startPos.x, startPos.y)
    if (first) applyTarget(first, startPos.x, startPos.y)
    if (target && pointerId >= 0) {
      try {
        target.setPointerCapture(pointerId)
        activePointerId = pointerId
      } catch {
        // Safari occasionally rejects capture on transformed elements;
        // drag still works via the document-level listeners.
      }
    }
  }

  function handlePointerDown(index: number, e: PointerEvent) {
    if (!enabled.value) return
    if (e.button !== undefined && e.button !== 0) return

    startPos = { x: e.clientX, y: e.clientY }
    lastPointer = { x: e.clientX, y: e.clientY }
    pointerMoved = false

    if (e.pointerType === 'mouse') {
      e.preventDefault()
      beginDrag(index, e.pointerId, e.target as HTMLElement | null)
      return
    }

    pendingIndex = index
    pendingPointerId = e.pointerId
    pendingTarget = e.target as HTMLElement | null
    holdTimer = setTimeout(() => {
      if (pendingIndex === -1) return
      beginDrag(pendingIndex, pendingPointerId, pendingTarget)
      pendingIndex = -1
      pendingPointerId = -1
      pendingTarget = null
      holdTimer = null
      navigator.vibrate?.(30)
    }, touchHoldMs)
  }

  function isDragged(index: number) {
    return dragState.isDragging && dragState.sourceIndex === index
  }

  onMounted(() => {
    document.addEventListener('pointermove', onPointerMove)
    document.addEventListener('pointerup', onPointerUp)
    document.addEventListener('pointercancel', reset)
    document.addEventListener('lostpointercapture', onLostPointerCapture)
    document.addEventListener('contextmenu', onContextMenu)
  })

  onBeforeUnmount(() => {
    document.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('pointerup', onPointerUp)
    document.removeEventListener('pointercancel', reset)
    document.removeEventListener('lostpointercapture', onLostPointerCapture)
    document.removeEventListener('contextmenu', onContextMenu)
    if (holdTimer) clearTimeout(holdTimer)
    if (autoScrollRaf) cancelAnimationFrame(autoScrollRaf)
  })

  return { dragState, handlePointerDown, isDragged }
}

// ---------------------------------------------------------------------------
// Projection helper
// ---------------------------------------------------------------------------

/** Minimum shape the projection helper needs from each visible
 *  entry: a stable original index (so the caller can compare it
 *  against `dragState.sourceIndex` / `hoverIndex`) plus the entry's
 *  column and row spans on the fixed-unit grid lattice. */
export interface ProjectableEntry {
  originalIndex: number
  colSpan: number
  rowSpan: number
}

/** A (row, col) cell on the grid lattice, both 0-based and measured
 *  in lattice units (not pixels). */
export interface GridCell {
  row: number
  col: number
}

/**
 * Place entries onto a fixed-unit grid lattice, mirroring CSS
 * `grid-auto-flow: row dense`: each entry takes the earliest
 * (row-major, then column) free slot that fits its `colSpan` x
 * `rowSpan` footprint without overlapping an already-placed entry.
 * That backfill is what lets two short (rowSpan 1) widgets stack to
 * the left of one tall (rowSpan 2) widget. Returns a map of
 * `originalIndex` to its top-left lattice cell.
 *
 * Pure: the caller runs it for both the original and projected entry
 * orders and differences the two cells to get each widget's lattice
 * displacement, which becomes an exact pixel transform because every
 * lattice row is the same height (unlike the old `auto-rows-min`
 * model where row height depended on content).
 */
export function packGrid(
  entries: readonly ProjectableEntry[],
  cols: number,
): Map<number, GridCell> {
  const out = new Map<number, GridCell>()
  if (cols < 1) return out

  // Occupancy lattice, grown lazily one row at a time.
  const occ: boolean[][] = []
  const ensureRow = (r: number) => {
    while (occ.length <= r) occ.push(new Array<boolean>(cols).fill(false))
  }
  const fits = (r: number, c: number, w: number, h: number): boolean => {
    if (c + w > cols) return false
    for (let i = r; i < r + h; i++) {
      ensureRow(i)
      for (let j = c; j < c + w; j++) if (occ[i][j]) return false
    }
    return true
  }
  const occupy = (r: number, c: number, w: number, h: number) => {
    for (let i = r; i < r + h; i++) {
      ensureRow(i)
      for (let j = c; j < c + w; j++) occ[i][j] = true
    }
  }

  for (const e of entries) {
    const w = Math.max(1, Math.min(e.colSpan, cols))
    const h = Math.max(1, e.rowSpan)
    let placed = false
    for (let r = 0; !placed; r++) {
      for (let c = 0; c + w <= cols; c++) {
        if (fits(r, c, w, h)) {
          occupy(r, c, w, h)
          out.set(e.originalIndex, { row: r, col: c })
          placed = true
          break
        }
      }
    }
  }
  return out
}

/**
 * The index in the source-removed list where the source will land
 * on commit. Splice the source out at `srcVIdx`, then splice it back
 * in at the returned index to produce the projected list. Returns
 * `null` for no-op snap zones (source ends up where it started).
 */
export function projectedTargetIndex(
  visibleEntries: readonly ProjectableEntry[],
  sourceIndex: number,
  hoverIndex: number,
  dropBefore: boolean,
): number | null {
  if (sourceIndex < 0 || hoverIndex < 0 || sourceIndex === hoverIndex) return null
  const srcVIdx = visibleEntries.findIndex((e) => e.originalIndex === sourceIndex)
  const hvrVIdx = visibleEntries.findIndex((e) => e.originalIndex === hoverIndex)
  if (srcVIdx === -1 || hvrVIdx === -1) return null
  let to = dropBefore ? hvrVIdx : hvrVIdx + 1
  if (srcVIdx < hvrVIdx) to -= 1
  if (to === srcVIdx) return null
  return to
}
