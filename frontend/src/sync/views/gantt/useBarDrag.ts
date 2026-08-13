/**
 * Continuous drag geometry for gantt bars: move the whole bar, or
 * resize either edge, snapped to whole days with a live preview.
 *
 * Kept separate from `useDragDrop` (the lane-drop machinery the
 * kanban and the unscheduled tray use) because a bar drag is
 * continuous geometry with no lane and no click threshold on the
 * handles; the two models share nothing but pointer events.
 *
 * Axis-agnostic: desktop runs time on X, the vertical timeline on Y.
 * The drag model is identical; only which client coordinate maps to
 * a day changes.
 *
 * Semantics ("respect the data" rule):
 * - `due` handle: moves `due_date`, clamped after the start.
 * - `start` handle: moves `start_date`, clamped before the due.
 *   Grabbing it on a bar whose left edge was the created_at
 *   fallback PROMOTES the ticket to a planned start.
 * - body drag: translates both dates by the snapped day delta
 *   (also promotes an unplanned start). Only offered on
 *   non-terminal bars; a small threshold keeps click-to-open
 *   working.
 *
 * Escape cancels; the edge scroller nudges the canvas when the
 * pointer nears the panel edges; the preview ref is the single
 * source the renderer reads (bars, ghost outline, date chip).
 */
import { ref, type Ref } from 'vue'
import { addDays } from 'date-fns'
import { createDragEdgeScroller } from '@/composables/useDragEdgeScroll'
import { daysBetween } from '@/composables/useGanttViewport'
import { createTouchHold } from '@/sync/views/touchHold'

export type BarDragMode = 'move' | 'start' | 'due'
export type BarDragAxis = 'x' | 'y'

export interface BarDragPreview {
  cardId: number
  mode: BarDragMode
  /** Live (snapped) span under the cursor. */
  start: Date
  end: Date
  /** Original span, for the ghost outline. */
  origStart: Date
  origEnd: Date
}

export interface BarDragCommit {
  cardId: number
  mode: BarDragMode
  start: Date
  end: Date
}

/**
 * Day offset (from canvasStart) under a client coordinate on the
 * time axis. Pure so vertical and horizontal projection share one
 * snap rule.
 */
export function dayOffsetAt(
  client: number,
  origin: number,
  pxPerDay: number,
): number {
  return Math.round((client - origin) / pxPerDay)
}

export function useBarDrag(options: {
  pxPerDay: Ref<number>
  canvasStart: Ref<Date>
  /** Timeline body; its rect's time-axis edge is canvas offset 0. */
  bodyEl: Ref<HTMLElement | null>
  /** The scroll container, for edge auto-scroll. */
  scroller: Ref<HTMLElement | null>
  /**
   * Which client axis is time. Default `x` preserves the desktop
   * board; the vertical timeline passes `y`.
   */
  axis?: BarDragAxis
  onCommit: (commit: BarDragCommit) => void
  /** Fired when a press becomes an actual drag (dismiss hover). */
  onDragStart?: () => void
}) {
  const axis: BarDragAxis = options.axis ?? 'x'
  const preview = ref<BarDragPreview | null>(null)

  let pointerId: number | null = null
  let mode: BarDragMode | null = null
  let dragging = false
  let downX = 0
  let downY = 0
  let downDay = 0
  let suppressClick = false
  // Hold-to-drag on touch, shared with the kanban. Resize handles are exempt:
  // grabbing one is already unambiguous, so only the bar BODY waits for a hold.
  const touchHold = createTouchHold(() => dragging)

  const edgeScroller = createDragEdgeScroller({
    getTargets: () => {
      const el = options.scroller.value
      // Vertical canvas pans on both axes once concurrency exceeds the
      // legibility floor; desktop gantt only edge-scrolls time (X).
      const axes = axis === 'y' ? ('both' as const) : ('x' as const)
      return el ? [{ el, axes }] : []
    },
    onTick: (clientX, clientY) => {
      // Scrolling under a stationary pointer changes the day under
      // it; re-derive the preview.
      if (dragging) applyPointer(axisCoord(clientX, clientY))
    },
  })

  function axisCoord(clientX: number, clientY: number): number {
    return axis === 'y' ? clientY : clientX
  }

  /** Day offset (from canvasStart) under the pointer on the time axis. */
  function dayAt(client: number): number {
    const body = options.bodyEl.value
    if (!body) return 0
    const rect = body.getBoundingClientRect()
    const origin = axis === 'y' ? rect.top : rect.left
    return dayOffsetAt(client, origin, options.pxPerDay.value)
  }

  function begin(
    m: BarDragMode,
    bar: { cardId: number; start: Date; end: Date },
    event: PointerEvent,
  ): void {
    if (event.button !== 0) return
    pointerId = event.pointerId
    mode = m
    downX = event.clientX
    downY = event.clientY
    downDay = dayAt(axisCoord(event.clientX, event.clientY))
    dragging = m !== 'move' // handles drag immediately; body waits for threshold
    preview.value = {
      cardId: bar.cardId,
      mode: m,
      start: bar.start,
      end: bar.end,
      origStart: bar.start,
      origEnd: bar.end,
    }
    if (dragging) {
      options.onDragStart?.()
      edgeScroller.start()
      edgeScroller.update(downX, downY)
    }
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    window.addEventListener('pointercancel', cancel)
    window.addEventListener('keydown', onKeyDown)
    touchHold.begin(event, () => {
      if (mode !== 'move' || dragging) return
      dragging = true
      options.onDragStart?.()
      edgeScroller.start()
      // Seed the pointer, or the scroller pans from its (0, 0) default.
      edgeScroller.update(downX, downY)
      document.body.style.userSelect = 'none'
    })
  }

  function applyPointer(client: number): void {
    const p = preview.value
    if (!p || !mode) return
    if (mode === 'due') {
      let end = addDays(options.canvasStart.value, dayAt(client))
      if (end.getTime() <= p.origStart.getTime()) end = addDays(p.origStart, 1)
      preview.value = { ...p, end }
    } else if (mode === 'start') {
      let start = addDays(options.canvasStart.value, dayAt(client))
      if (start.getTime() >= p.origEnd.getTime()) start = addDays(p.origEnd, -1)
      preview.value = { ...p, start }
    } else {
      const delta = dayAt(client) - downDay
      preview.value = {
        ...p,
        start: addDays(p.origStart, delta),
        end: addDays(p.origEnd, delta),
      }
    }
  }

  function onPointerMove(event: PointerEvent): void {
    if (event.pointerId !== pointerId) return
    if (!dragging && mode === 'move') {
      // On touch, movement before the hold completes is a pan, not a drag.
      // Abandon the press so the browser keeps the gesture.
      if (touchHold.isTouch) {
        if (touchHold.exceedsSlop(event.clientX - downX, event.clientY - downY)) teardown()
        return
      }
      const delta = axis === 'y' ? event.clientY - downY : event.clientX - downX
      if (Math.abs(delta) <= 4) return
      dragging = true
      options.onDragStart?.()
      edgeScroller.start()
      document.body.style.userSelect = 'none'
    }
    if (!dragging) return
    applyPointer(axisCoord(event.clientX, event.clientY))
    edgeScroller.update(event.clientX, event.clientY)
  }

  function onPointerUp(event: PointerEvent): void {
    if (event.pointerId !== pointerId) return
    const p = preview.value
    const wasDragging = dragging
    teardown()
    if (!p || !wasDragging) return
    // A completed drag must not read as a click on release.
    suppressClick = true
    setTimeout(() => {
      suppressClick = false
    }, 0)
    const changed =
      daysBetween(p.origStart, p.start) !== 0 || daysBetween(p.origEnd, p.end) !== 0
    if (changed) {
      options.onCommit({ cardId: p.cardId, mode: p.mode, start: p.start, end: p.end })
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return
    event.preventDefault()
    cancel()
  }

  function cancel(): void {
    teardown()
  }

  function teardown(): void {
    touchHold.end()
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', onPointerUp)
    window.removeEventListener('pointercancel', cancel)
    window.removeEventListener('keydown', onKeyDown)
    edgeScroller.stop()
    document.body.style.userSelect = ''
    preview.value = null
    pointerId = null
    mode = null
    dragging = false
  }

  /** True right after a body drag committed, so the bar's click
   * handler can swallow the trailing click event. */
  function shouldSuppressClick(): boolean {
    return suppressClick
  }

  return { preview, begin, shouldSuppressClick }
}
