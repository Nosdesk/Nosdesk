/**
 * Pointer-event sortable list with deferred reorder + projected
 * drop-position indicator (see docs/plans/v1-dashboard-dnd-spec.md).
 *
 * HTML5 DnD is avoided across the app (touch support, dragleave-on-
 * children, crippled drag image). This composable uses Pointer
 * Events — same pattern as `useKanbanDragDrop`.
 *
 * Rendering contract:
 *   - The view renders items with `data-sortable-index="${n}"` on
 *     each item's root element. `n` is the ORIGINAL index into the
 *     underlying array.
 *   - The view wires `@pointerdown` on a drag handle inside each
 *     item to `handlePointerDown(index, event)`.
 *   - On successful drop the composable calls `onReorder(from, to)`.
 *   - The view DOES NOT reorder its rendered output during the drag.
 *     Siblings stay at their original positions; a separate indicator
 *     marks the projected post-drop slot. The grid commits the move
 *     once on `pointerup` and the existing FLIP transition animates
 *     the reflow.
 *
 * State the view consumes:
 *   - `dragState.isDragging`, `sourceIndex`, `hoverIndex`, `dropBefore` —
 *     the classic engine fields. `hoverIndex` + `dropBefore` are the
 *     intent for the next commit; the caller projects them into a
 *     visual landing slot via `computeDropTargetGap`.
 *   - `dragState.renderedColumns` — snapshotted at drag-start from the
 *     grid container's computed `grid-template-columns`. Lets the
 *     indicator math collapse cleanly under the `grid-cols-1` mobile
 *     layout without a parallel code path.
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
   *  drag-start. Used by `computeDropTargetGap` to clamp the
   *  dragged widget's effective span — at xl+ this is typically
   *  3; below xl the grid collapses to 1 and the math degrades
   *  cleanly to a single-column list. */
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
    const el = getGridEl?.()
    if (!el) return 1
    const tracks = getComputedStyle(el).gridTemplateColumns.trim()
    if (!tracks || tracks === 'none') return 1
    return tracks.split(/\s+/).filter(Boolean).length
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
// Projected-drop-position helper
// ---------------------------------------------------------------------------

/** Minimum shape the projection helper needs from each visible
 *  entry: a stable original index (so it can compare against
 *  `dragState.sourceIndex` / `hoverIndex`) and the entry's column
 *  span in the flow grid. */
export interface ProjectableEntry {
  originalIndex: number
  colSpan: number
}

/** The visual landing slot the drop indicator renders.
 *
 *   - `columnStart` is 1-based, matching CSS grid line numbers, so
 *     the caller can compose it into a `grid-column-start` style or
 *     use it to compute a pixel x-offset against the grid's
 *     bounding rect.
 *   - `rowIndex` is 0-based and counted in flow rows (each row
 *     fits up to `renderedColumns` cells of total span).
 *   - `colSpan` is the dragged entry's clamped effective span,
 *     `min(storedColSpan, renderedColumns)`.
 *
 *   The caller turns this into an absolute pixel position by
 *   measuring the grid container and reading the gap from
 *   `getComputedStyle`.
 */
export interface DropTargetGap {
  columnStart: number
  rowIndex: number
  colSpan: number
}

/**
 * Project the post-drop layout to figure out where the dragged
 * widget will visually land. Performs the same array splice the
 * Pinia store's `move(from, to)` will run on commit, then assigns
 * columns in a single linear pass.
 *
 * Returns `null` when no meaningful projection exists (no drag in
 * progress, source not in visible entries, source === hover, or
 * `renderedColumns < 1`).
 *
 * The function is pure so the caller can drive it from a Vue
 * `computed()` and React to changes in any of the inputs cheaply.
 */
export function computeDropTargetGap(
  visibleEntries: readonly ProjectableEntry[],
  sourceIndex: number,
  hoverIndex: number,
  dropBefore: boolean,
  renderedColumns: number,
): DropTargetGap | null {
  if (sourceIndex < 0 || hoverIndex < 0) return null
  if (renderedColumns < 1) return null

  const srcVIdx = visibleEntries.findIndex((e) => e.originalIndex === sourceIndex)
  const hvrVIdx = visibleEntries.findIndex((e) => e.originalIndex === hoverIndex)
  if (srcVIdx === -1 || hvrVIdx === -1 || srcVIdx === hvrVIdx) return null

  // The splice the store will run on commit.
  const projected = visibleEntries.slice()
  const [moved] = projected.splice(srcVIdx, 1)
  let to = dropBefore ? hvrVIdx : hvrVIdx + 1
  if (srcVIdx < hvrVIdx) to -= 1
  projected.splice(to, 0, moved)

  const effectiveSpan = Math.max(1, Math.min(moved.colSpan, renderedColumns))

  // Single linear pass assigning column positions in the flow grid.
  // When the next entry won't fit on the current row it wraps to the
  // next row's leading edge, mirroring CSS grid's auto-flow behaviour.
  let columnStart = 1
  let rowIndex = 0
  for (let i = 0; i < projected.length; i++) {
    const entry = projected[i]
    const span = Math.max(1, Math.min(entry.colSpan, renderedColumns))
    if (columnStart + span - 1 > renderedColumns) {
      // Wrap to the next row's leading edge before placing this entry.
      columnStart = 1
      rowIndex += 1
    }
    if (i === to) {
      // The dragged widget. Indicator landing slot found.
      return { columnStart, rowIndex, colSpan: effectiveSpan }
    }
    columnStart += span
    if (columnStart > renderedColumns) {
      columnStart = 1
      rowIndex += 1
    }
  }
  return null
}
