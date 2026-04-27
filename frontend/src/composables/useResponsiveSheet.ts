/**
 * Shared bottom-sheet behaviour for floating surfaces that
 * adapt across the desktop/mobile breakpoint. Consumed by both
 * `<ResponsivePanel>` (side-panel-vs-bottom-sheet for secondary
 * page content) and `<ResponsiveMenu>` (popover-vs-bottom-sheet
 * for action menus and dropdowns).
 *
 * Owns:
 *   - reactive `isMobile` from a configurable media query
 *   - drag offset + drag state for the sheet handle
 *   - dismiss-on-drag-down past a threshold
 *   - close-on-breakpoint-cross policy (we dismiss rather than
 *     morph; an animated 320px-panel → 75vh-sheet transition
 *     always reads worse than reopen-if-you-want)
 *
 * Does NOT own:
 *   - body scroll lock (consumers compose useScrollLock
 *     independently — different surfaces want subtly different
 *     timing)
 *   - the actual rendering / template (consumers decide what
 *     the sheet contains and how its chrome looks)
 */
import { onMounted, onScopeDispose, ref, watch, type Ref } from 'vue'

interface UseResponsiveSheetOptions {
  /** Reactive open flag from the consumer. We watch it so we
   * can reset drag state on each open and react to
   * close-while-dragging cleanly. */
  open: Ref<boolean>
  /** CSS media query that defines "desktop". When it matches,
   * `isMobile` is false and the consumer should render the
   * desktop variant. Defaults to Tailwind's `md` breakpoint. */
  breakpoint?: string
  /** Called when the user drags the sheet down past the
   * dismiss threshold. The composable doesn't manage `open`
   * itself — the parent owns that state and decides what to
   * do (commonly: set `open = false`). */
  onDismiss: () => void
  /** How far the sheet must travel downward (in pixels) on
   * pointerup to count as a dismiss. Defaults to 100, which
   * is generous enough to ignore small thumb wobble but tight
   * enough that an intentional swipe down always lands. */
  dismissThreshold?: number
}

export interface UseResponsiveSheetReturn {
  isMobile: Readonly<Ref<boolean>>
  /** Pixels the sheet should be translated downward. Apply via
   * inline style on the sheet's transform. Reset to 0 on each
   * open. */
  dragOffset: Readonly<Ref<number>>
  /** True while the user is actively pressing the handle —
   * useful for cursor styling and pausing the slide-back
   * transition. */
  isDragging: Readonly<Ref<boolean>>
  /** Spread onto the drag-handle element via `v-on`. The
   * handle, NOT the sheet content — letting the user drag
   * anywhere conflicts with scroll-the-content gestures. */
  handleListeners: {
    pointerdown: (event: PointerEvent) => void
    pointermove: (event: PointerEvent) => void
    pointerup: (event: PointerEvent) => void
    pointercancel: (event: PointerEvent) => void
  }
}

export function useResponsiveSheet(
  opts: UseResponsiveSheetOptions,
): UseResponsiveSheetReturn {
  const breakpoint = opts.breakpoint ?? '(min-width: 768px)'
  const dismissThreshold = opts.dismissThreshold ?? 100

  const isMobile = ref(false)
  const dragOffset = ref(0)
  const isDragging = ref(false)
  let dragStartY = 0

  // -----------------------------------------------------------
  // Breakpoint reactivity
  // -----------------------------------------------------------
  let mql: MediaQueryList | null = null

  function syncBreakpoint(e: MediaQueryListEvent | MediaQueryList) {
    isMobile.value = !e.matches
  }

  onMounted(() => {
    if (typeof window === 'undefined') return
    mql = window.matchMedia(breakpoint)
    syncBreakpoint(mql)
    mql.addEventListener('change', syncBreakpoint)
  })

  onScopeDispose(() => {
    mql?.removeEventListener('change', syncBreakpoint)
    mql = null
  })

  // Cross-breakpoint dismiss: avoid morphing a desktop popover
  // into a mobile sheet (or vice versa) — the geometries are
  // different enough that the animation always looks broken.
  watch(isMobile, (_now, prev) => {
    if (opts.open.value && prev !== undefined) opts.onDismiss()
  })

  // Reset drag state on each open so a previous drag doesn't
  // leak into a fresh open.
  watch(opts.open, (open) => {
    if (open) dragOffset.value = 0
  })

  // -----------------------------------------------------------
  // Drag handlers — bind to the sheet's drag handle, not the
  // content. Drag only goes down (the Math.max clamp); past
  // the threshold on release dismisses, otherwise springs back.
  // -----------------------------------------------------------
  function pointerdown(event: PointerEvent) {
    if (event.button !== 0) return
    isDragging.value = true
    dragStartY = event.clientY
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  }

  function pointermove(event: PointerEvent) {
    if (!isDragging.value) return
    dragOffset.value = Math.max(0, event.clientY - dragStartY)
  }

  function pointerup() {
    if (!isDragging.value) return
    isDragging.value = false
    if (dragOffset.value > dismissThreshold) {
      opts.onDismiss()
    }
    // Whether dismissed or not, reset. On dismiss the sheet
    // unmounts via the parent's open-flip; the spring-back
    // animation only fires on cancelled drags.
    dragOffset.value = 0
  }

  function pointercancel() {
    pointerup()
  }

  return {
    isMobile,
    dragOffset,
    isDragging,
    handleListeners: { pointerdown, pointermove, pointerup, pointercancel },
  }
}
