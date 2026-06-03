/**
 * Pointer-event sortable list with live preview reorder.
 *
 * HTML5 DnD is avoided across the app (touch support, dragleave-on-
 * children, crippled drag image). This composable uses Pointer
 * Events — same pattern as `useKanbanDragDrop`.
 *
 * Rendering contract:
 *   - The view renders items with `data-sortable-index="${n}"` on
 *     each item's root element. `n` is the ORIGINAL index into the
 *     underlying array; the view is free to reorder the rendered
 *     output via `previewOrder()`.
 *   - The view wires `@pointerdown` on a drag handle inside each
 *     item to `handlePointerDown(index, event)`.
 *   - On successful drop the composable calls `onReorder(from, to)`.
 *
 * Hit-testing:
 *   - Every sortable item's rect is frozen at drag start and reused
 *     for the duration of the drag. That decouples hit-testing from
 *     the preview reorder — we hit-test against the original layout
 *     even after widgets shift on screen — which prevents the cursor
 *     from chasing the moving preview.
 *   - Rect containment is the selector (full-widget target area,
 *     Fitts's-Law friendly). Within a target, the split axis is
 *     viewport-aware: on multi-column layouts (>= Tailwind xl,
 *     1280px) the X-midline splits before/after; on single-column
 *     layouts (mobile and narrow desktop) the Y-midline splits.
 *     Using the wrong axis at the wrong viewport produces the
 *     "drop indicator chasing my cursor sideways" feel reported in
 *     the interaction-model design pass — a horizontal-axis split
 *     on a vertical list is meaningless.
 *   - Cursor in a gap between widgets: the last known target is
 *     retained so the preview doesn't flicker off.
 */
import { onBeforeUnmount, onMounted, reactive, type Ref } from 'vue'

export interface PointerSortableOptions {
  enabled: Ref<boolean>
  onReorder: (fromIndex: number, toIndex: number) => void
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
}

const ITEM_ATTR = 'data-sortable-index'

export function usePointerSortable(options: PointerSortableOptions) {
  const { enabled, onReorder, clickThreshold = 5, touchHoldMs = 400 } = options

  const dragState = reactive<DragState>({
    isDragging: false,
    sourceIndex: -1,
    hoverIndex: -1,
    dropBefore: true,
  })

  let frozenRects: FrozenRect[] = []
  let startPos = { x: 0, y: 0 }
  let pendingIndex = -1
  let pendingPointerId = -1
  let pendingTarget: HTMLElement | null = null
  let holdTimer: ReturnType<typeof setTimeout> | null = null
  let pointerMoved = false

  function reset() {
    if (holdTimer) {
      clearTimeout(holdTimer)
      holdTimer = null
    }
    frozenRects = []
    pendingIndex = -1
    pendingPointerId = -1
    pendingTarget = null
    pointerMoved = false
    dragState.isDragging = false
    dragState.sourceIndex = -1
    dragState.hoverIndex = -1
    dragState.dropBefore = true
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
  }

  /** Widget rect containing (x, y), excluding the source. `null` if
   *  the cursor is in a gap — callers should leave the last known
   *  hover state untouched in that case. */
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

  /** The bottom-most visible sibling, excluding the source. Used when
   *  the cursor drops past the end of the list — there is no "row"
   *  concept in the grid, so "move to a new row" really means "insert
   *  after the last widget in the flat order." */
  function findTrailingTarget(): FrozenRect | null {
    let chosen: FrozenRect | null = null
    for (const fr of frozenRects) {
      if (fr.index === dragState.sourceIndex) continue
      if (!chosen || fr.rect.bottom > chosen.rect.bottom) chosen = fr
    }
    return chosen
  }

  /** Tailwind's `xl` breakpoint. Below this width the dashboard
   *  grid collapses to a single column and a vertical drag is the
   *  only meaningful axis; above it, the grid lays out 2–3 columns
   *  and a horizontal split tells you which side of a neighbour
   *  you're targeting. Cached once per drag (in `freezeRects`) so
   *  the axis doesn't flip mid-gesture if the user happens to
   *  resize during a drag. */
  const MULTI_COLUMN_MIN_WIDTH = 1280
  let multiColumnLayout = false

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

    const dx = Math.abs(e.clientX - startPos.x)
    const dy = Math.abs(e.clientY - startPos.y)
    if (dx > clickThreshold || dy > clickThreshold) pointerMoved = true

    const target = findTarget(e.clientX, e.clientY)
    if (target) {
      applyTarget(target, e.clientX, e.clientY)
      return
    }

    // Cursor is outside every sibling's rect. If it's below all of
    // them, treat the drop as "append after the last widget" — this
    // is how a user drags an item into a new (empty) row at the end.
    // Anywhere else (gap between rows, above all widgets) we leave
    // the last-known hover intact so the preview doesn't flicker.
    const trailing = findTrailingTarget()
    if (trailing && e.clientY > trailing.rect.bottom) {
      dragState.hoverIndex = trailing.index
      dragState.dropBefore = false
    }
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

    if (!moved || sourceIndex === -1 || hoverIndex === -1) return
    if (sourceIndex === hoverIndex) return

    let to = dropBefore ? hoverIndex : hoverIndex + 1
    if (to > sourceIndex) to -= 1
    if (to === sourceIndex) return

    onReorder(sourceIndex, to)
  }

  function beginDrag(index: number, pointerId: number, target: HTMLElement | null) {
    freezeRects()
    dragState.isDragging = true
    dragState.sourceIndex = index
    const first = findTarget(startPos.x, startPos.y)
    if (first) applyTarget(first, startPos.x, startPos.y)
    if (target && pointerId >= 0) {
      try {
        target.setPointerCapture(pointerId)
      } catch {
        /* Safari occasionally rejects capture on transformed elements;
           drag still works via the document-level listeners. */
      }
    }
  }

  function handlePointerDown(index: number, e: PointerEvent) {
    if (!enabled.value) return
    if (e.button !== undefined && e.button !== 0) return

    startPos = { x: e.clientX, y: e.clientY }
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

  /** Given entries keyed by `originalIndex`, return a reordered copy
   *  reflecting where the dragged entry will land. Unchanged when no
   *  drag is in progress. */
  function previewOrder<T extends { originalIndex: number }>(entries: T[]): T[] {
    if (!dragState.isDragging) return entries
    const { sourceIndex, hoverIndex, dropBefore } = dragState
    if (sourceIndex === -1 || hoverIndex === -1) return entries

    const srcVIdx = entries.findIndex((e) => e.originalIndex === sourceIndex)
    const hvrVIdx = entries.findIndex((e) => e.originalIndex === hoverIndex)
    if (srcVIdx === -1 || hvrVIdx === -1 || srcVIdx === hvrVIdx) return entries

    const out = entries.slice()
    const [moved] = out.splice(srcVIdx, 1)
    let to = dropBefore ? hvrVIdx : hvrVIdx + 1
    if (srcVIdx < hvrVIdx) to -= 1
    out.splice(to, 0, moved)
    return out
  }

  onMounted(() => {
    document.addEventListener('pointermove', onPointerMove)
    document.addEventListener('pointerup', onPointerUp)
    document.addEventListener('pointercancel', reset)
  })

  onBeforeUnmount(() => {
    document.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('pointerup', onPointerUp)
    document.removeEventListener('pointercancel', reset)
    if (holdTimer) clearTimeout(holdTimer)
  })

  return { dragState, handlePointerDown, isDragged, previewOrder }
}
