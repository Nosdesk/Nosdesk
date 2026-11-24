// Drag ordering for rows in a list, on the app's own drag helper
// (`./drag`), which brings the click threshold, hold-to-drag on touch,
// Escape and edge scrolling. This adds what a reorder needs on top: the
// lane under the pointer resolves to an insertion slot from the rows'
// midpoints, and the slot's edge is kept as a line to draw.
//
// A lane is any element with `data-lane`; its rows are the `data-row`
// elements inside it. Rows do not move during the drag; the line shows
// where the drop lands.
import { computed, reactive, ref, watch } from 'vue'
import { useDragDrop } from './drag'

export interface DropLine {
  top: number
  left: number
  width: number
}

export interface RowDrop {
  id: number
  lane: string
  /** Index among the lane's rows, the moved row left out. */
  slot: number
}

export interface RowBox {
  id: number
  top: number
  bottom: number
}

/**
 * Where a drop at `y` lands among a lane's rows: the slot (the index
 * among the rows other than the moved one), whether that is where the
 * row already is, and the y of the edge to draw the line on. `laneBottom`
 * places the line in an empty lane.
 */
export function insertionAt(
  rows: readonly RowBox[],
  movedId: number,
  y: number,
  laneBottom: number,
): { slot: number; same: boolean; edge: number } {
  const others = rows.filter((row) => row.id !== movedId)
  let slot = 0
  for (const row of others) {
    if (y > row.top + (row.bottom - row.top) / 2) slot += 1
  }
  const from = rows.findIndex((row) => row.id === movedId)
  const edge =
    slot < others.length
      ? others[slot].top
      : others.length > 0
        ? others[others.length - 1].bottom
        : laneBottom
  return { slot, same: from === slot, edge }
}

export function useRowReorder(options: {
  /** The element that holds the lanes; also the one that scrolls. */
  root: () => HTMLElement | null
  onDrop: (drop: RowDrop) => void
  onClick?: (id: number) => void
}) {
  const line = ref<DropLine | null>(null)
  /** True from an activated drag until the pointer is released and the
   *  events that trail it (click, contextmenu) have passed. */
  const settling = ref(false)
  const last = reactive({ pointerType: 'mouse' })

  function resolve(x: number, y: number): string | null {
    const root = options.root()
    const id = drag.state.draggedCardIds[0]
    if (!root || id == null) return null
    for (const lane of root.querySelectorAll<HTMLElement>('[data-lane]')) {
      const box = lane.getBoundingClientRect()
      if (y < box.top || y > box.bottom || x < box.left || x > box.right) continue
      const rows = [...lane.querySelectorAll<HTMLElement>('[data-row]')].map((row) => {
        const rect = row.getBoundingClientRect()
        return { id: Number(row.dataset.row), top: rect.top, bottom: rect.bottom }
      })
      const at = insertionAt(rows, id, y, box.bottom)
      // Where it already is: no line, and the drop changes nothing.
      if (at.same) {
        line.value = null
        return `${lane.dataset.lane}:${at.slot}:same`
      }
      line.value = { top: at.edge, left: box.left, width: box.width }
      return `${lane.dataset.lane}:${at.slot}`
    }
    line.value = null
    return null
  }

  const drag = useDragDrop({
    resolveLaneAt: resolve,
    onDrop: ({ cardIds, targetLane }) => {
      const [lane, slot, same] = targetLane.split(':')
      if (same) return
      options.onDrop({ id: cardIds[0], lane, slot: Number(slot) })
    },
    onClick: (id) => {
      if (!settling.value) options.onClick?.(id)
    },
    getEdgeScrollTargets: () => {
      const el = options.root()
      return el ? [{ el, axes: 'y' }] : []
    },
  })

  watch(
    () => drag.state.isDragging,
    (dragging) => {
      if (dragging) {
        settling.value = true
        // The lift, where the device can say so.
        if (last.pointerType === 'touch') navigator.vibrate?.(8)
      } else {
        line.value = null
        window.setTimeout(() => (settling.value = false), 300)
      }
    },
  )

  /** A press on the row: a mouse drags past the click threshold, a
   *  finger has to hold first, so a swipe still scrolls the list. A press
   *  on a control inside the row is the control's, unless the control is
   *  the row itself (`data-row-press`, a card that is one button). */
  function onRowDown(id: number, event: PointerEvent): void {
    const control = (event.target as HTMLElement).closest('button, a, input, [data-no-drag]')
    if (control && !control.hasAttribute('data-row-press')) return
    last.pointerType = event.pointerType
    drag.onPointerDown(id, event)
  }

  /** A press on the grip: the grip takes no scroll (`touch-action:
   *  none`), so a finger drags it at once, the way a mouse does. */
  function onGripDown(id: number, event: PointerEvent): void {
    last.pointerType = event.pointerType
    event.preventDefault()
    drag.onPointerDown(id, {
      button: event.button,
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      pointerType: 'mouse',
    } as PointerEvent)
  }

  return {
    state: drag.state,
    line,
    settling,
    draggedId: computed(() => (drag.state.isDragging ? (drag.state.draggedCardIds[0] ?? null) : null)),
    /** The pointer that made the last press: `mouse`, `touch` or `pen`. */
    lastPointerType: () => last.pointerType,
    onRowDown,
    onGripDown,
  }
}
