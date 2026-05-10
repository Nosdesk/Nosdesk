/**
 * useResizableSidebar — drag-to-resize for the left navbar's
 * Tickets/Docs split. Surfaces the same `startResize` /
 * `equalizeHeights` / `ticketsHeight` API the original module
 * exposed; the gesture mechanics now live in `useDragGesture`
 * (shared with column resize and the split-pane divider).
 *
 * Strategy: LIVE update — the affected element is one sidebar
 * panel, so writing `style.maxHeight` on each rAF tick is
 * cheap and gives instant feedback. The composable handles
 * pointer capture, rAF coalescing, will-change hints, and
 * cleanup; this file owns the constraint math (min height per
 * section, max derived from the navbar's total height).
 */
import { ref, onMounted, onBeforeUnmount, type Ref, type ComputedRef } from 'vue'
import { useDragGesture } from '@/composables/useDragGesture'

const MIN_SECTION_HEIGHT = 60
const MIN_OTHER_SECTION_HEIGHT = 60
const RESIZER_HEIGHT = 8
const STORAGE_KEY = 'ticketsHeight'

export function useResizableSidebar(
  navbarRef: Ref<HTMLElement | null>,
  ticketsSectionRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>,
  _docsSectionRef: Ref<HTMLElement | null> | ComputedRef<HTMLElement | null>,
  _resizerRef: Ref<HTMLElement | null>,
) {
  const ticketsHeight = ref(200)
  const drag = useDragGesture()

  onMounted(() => {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored) {
      const parsed = parseInt(stored, 10)
      if (Number.isFinite(parsed)) ticketsHeight.value = parsed
    }
  })

  function applyHeight(newHeight: number): void {
    ticketsHeight.value = newHeight
    if (ticketsSectionRef.value) {
      ticketsSectionRef.value.style.maxHeight = `${newHeight}px`
    }
  }

  function startResize(event: PointerEvent): void {
    if (!ticketsSectionRef.value || !navbarRef.value) return

    const startHeight = ticketsSectionRef.value.offsetHeight
    document.body.classList.add('resize-active')

    drag.begin(event, {
      axis: 'y',
      startValue: startHeight,
      optimizationTarget: ticketsSectionRef.value,
      clamp: (raw) => {
        if (!navbarRef.value) return raw
        const totalHeight = navbarRef.value.offsetHeight
        const maxHeight = totalHeight - MIN_OTHER_SECTION_HEIGHT - RESIZER_HEIGHT
        return Math.max(MIN_SECTION_HEIGHT, Math.min(raw, maxHeight))
      },
      onUpdate: applyHeight,
      onCommit: (finalHeight) => {
        applyHeight(finalHeight)
        localStorage.setItem(STORAGE_KEY, String(finalHeight))
        document.body.classList.remove('resize-active')
      },
    })
  }

  function equalizeHeights(): void {
    if (!navbarRef.value || !ticketsSectionRef.value) return
    const totalHeight = navbarRef.value.getBoundingClientRect().height
    const equalHeight = Math.floor((totalHeight - RESIZER_HEIGHT) / 2)
    const finalHeight = Math.max(MIN_SECTION_HEIGHT, equalHeight)
    applyHeight(finalHeight)
    localStorage.setItem(STORAGE_KEY, String(finalHeight))
  }

  onBeforeUnmount(() => {
    document.body.classList.remove('resize-active')
  })

  return {
    ticketsHeight,
    isResizing: drag.isDragging,
    startResize,
    equalizeHeights,
  }
}
