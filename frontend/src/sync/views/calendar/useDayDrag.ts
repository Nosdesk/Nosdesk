// Carrying a job to another day. Pointer-based, like the board and My
// day: a mouse picks the job up once it has moved a few pixels, a finger
// by holding it. Days do not move while it is carried; the day under the
// pointer is marked, and the drop writes there.
import { onBeforeUnmount, reactive, ref } from 'vue'
import type { Day } from '@nosdesk/core/utils/ticketDates'

const SLOP = 5
const HOLD_MS = 350

export function useDayDrag(onDrop: (id: number, day: Day) => void) {
  const draggedId = ref<number | null>(null)
  const dropDay = ref<Day | null>(null)
  const position = reactive({ x: 0, y: 0 })
  /** A drag just ended: the click that follows it is not an open. */
  const justDragged = ref(false)

  let pending: { id: number; x: number; y: number; touch: boolean } | null = null
  let holdTimer: number | undefined

  function dayAt(x: number, y: number): Day | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-drop-day]')
    return el?.dataset.dropDay ?? null
  }

  function begin(x: number, y: number): void {
    if (!pending) return
    draggedId.value = pending.id
    position.x = x
    position.y = y
    dropDay.value = dayAt(x, y)
    document.body.style.userSelect = 'none'
  }

  function onMove(event: PointerEvent): void {
    if (!pending) return
    if (draggedId.value == null) {
      const moved = Math.hypot(event.clientX - pending.x, event.clientY - pending.y)
      if (pending.touch) {
        // A finger that moves before the hold is scrolling.
        if (moved > SLOP) cancel()
        return
      }
      if (moved < SLOP) return
      begin(event.clientX, event.clientY)
    }
    event.preventDefault()
    position.x = event.clientX
    position.y = event.clientY
    dropDay.value = dayAt(event.clientX, event.clientY)
  }

  function onTouchMove(event: TouchEvent): void {
    if (draggedId.value != null) event.preventDefault()
  }

  function finish(): void {
    const id = draggedId.value
    const day = dropDay.value
    if (id != null) {
      justDragged.value = true
      window.setTimeout(() => (justDragged.value = false), 0)
      if (day) onDrop(id, day)
    }
    cancel()
  }

  function cancel(): void {
    window.clearTimeout(holdTimer)
    pending = null
    draggedId.value = null
    dropDay.value = null
    document.body.style.userSelect = ''
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', finish)
    window.removeEventListener('pointercancel', cancel)
    window.removeEventListener('touchmove', onTouchMove)
    window.removeEventListener('keydown', onKey)
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key === 'Escape') cancel()
  }

  function onItemDown(id: number, event: PointerEvent): void {
    if (event.button !== 0) return
    cancel()
    const touch = event.pointerType === 'touch'
    pending = { id, x: event.clientX, y: event.clientY, touch }
    if (touch) holdTimer = window.setTimeout(() => pending && begin(pending.x, pending.y), HOLD_MS)
    window.addEventListener('pointermove', onMove, { passive: false })
    window.addEventListener('pointerup', finish)
    window.addEventListener('pointercancel', cancel)
    window.addEventListener('touchmove', onTouchMove, { passive: false })
    window.addEventListener('keydown', onKey)
  }

  onBeforeUnmount(cancel)

  return { draggedId, dropDay, position, justDragged, onItemDown }
}
