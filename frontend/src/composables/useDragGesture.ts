/**
 * useDragGesture — shared pointer-driven drag mechanics for
 * resize / drag-to-set surfaces (sidebar height, table column
 * width, split-pane width).
 *
 * Owns the mechanical bits that every drag-to-resize gesture
 * needs and that every consumer was previously reimplementing
 * (badly, in two of three cases):
 *   - pointer capture so the drag survives the cursor leaving
 *     the handle
 *   - `requestAnimationFrame` coalescing so per-frame updates
 *     run at most once per repaint, regardless of how often
 *     pointermove fires
 *   - `will-change` hint on a caller-supplied target while the
 *     gesture is active, removed on commit
 *   - `pointermove` / `pointerup` / `pointercancel` lifecycle
 *     management with guaranteed cleanup
 *   - axis + sign abstraction so a top-edge or right-edge
 *     handle behaves identically with a flag flip
 *
 * The consumer owns *what to render*. Two strategies cover all
 * three current sites:
 *   - LIVE: write the actual layout property (max-height,
 *     paneWidth) inside `onUpdate`. Cheap when the affected
 *     subtree is small.
 *   - GHOST: move a transform-only indicator inside `onUpdate`
 *     (translate3d → GPU compositor, no layout work) and only
 *     commit the real layout property in `onCommit`. The
 *     pattern AG Grid / Linear / Notion use for table column
 *     resize because reflowing N rows per frame stutters at
 *     scale.
 *
 * The composable doesn't pick the strategy; it just provides
 * the rAF-batched delta loop. The caller decides what to draw
 * during, and what to commit at the end.
 */
import { ref } from 'vue'

export interface DragGestureConfig {
  /** Which pointer coordinate the gesture tracks. */
  axis: 'x' | 'y'
  /** Sign convention. +1 (default): positive coordinate delta
   *  produces positive value delta. Use -1 for handles where
   *  dragging "back" should grow the value (e.g. a divider on
   *  the left edge of a right-anchored pane). */
  direction?: 1 | -1
  /** Snapshot of the value being driven at the moment the
   *  gesture starts. Subsequent `onUpdate` calls receive
   *  `startValue + (clamped) coordinate delta`. */
  startValue: number
  /** Element to flag with `will-change` for the duration of
   *  the gesture. Hints to the browser to promote the property
   *  to its own layer / pre-allocate compositor resources.
   *  Cleared on commit so it doesn't sit forever. */
  optimizationTarget?: HTMLElement | null
  /** Optional clamp applied to every emitted value. Min/max
   *  bounds, snapping, etc. */
  clamp?: (raw: number) => number
  /** Per-rAF-tick callback. May fire many times during a
   *  gesture; cheap operations only (transform writes, ref
   *  updates that don't trigger expensive reflows). */
  onUpdate: (value: number) => void
  /** Pointerup callback. Fires exactly once per gesture, with
   *  the final value. Persistence (localStorage, store
   *  mutations, sync writes) lives here. */
  onCommit: (value: number) => void
}

export function useDragGesture() {
  const isDragging = ref(false)

  /** Begin a drag. Call from a `@pointerdown` handler — the
   *  composable wires up move/up listeners on the target and
   *  manages their lifecycle until pointerup / cancel. */
  function begin(event: PointerEvent, config: DragGestureConfig): void {
    event.preventDefault()
    const target = event.currentTarget as HTMLElement | null
    if (!target) return

    target.setPointerCapture?.(event.pointerId)

    const direction = config.direction ?? 1
    const startCoord = config.axis === 'x' ? event.clientX : event.clientY
    let pendingValue = config.startValue
    let rafId: number | null = null

    isDragging.value = true
    if (config.optimizationTarget) {
      config.optimizationTarget.style.willChange =
        config.axis === 'x' ? 'width, transform' : 'height, transform'
    }

    const onMove = (e: PointerEvent) => {
      e.preventDefault()
      const coord = config.axis === 'x' ? e.clientX : e.clientY
      const raw = config.startValue + (coord - startCoord) * direction
      pendingValue = config.clamp ? config.clamp(raw) : raw

      // rAF coalescing: a single frame may receive many
      // pointermove events on high-polling pointers; we only
      // need the latest position when it's time to paint.
      if (rafId === null) {
        rafId = requestAnimationFrame(() => {
          rafId = null
          config.onUpdate(pendingValue)
        })
      }
    }

    const cleanup = (e: PointerEvent) => {
      target.releasePointerCapture?.(e.pointerId)
      target.removeEventListener('pointermove', onMove)
      target.removeEventListener('pointerup', cleanup)
      target.removeEventListener('pointercancel', cleanup)

      if (rafId !== null) {
        cancelAnimationFrame(rafId)
        rafId = null
      }

      isDragging.value = false
      if (config.optimizationTarget) {
        config.optimizationTarget.style.willChange = ''
      }

      // Always commit on release — even on `pointercancel` the
      // user has let go and expects the gesture's last visible
      // position to stick rather than snap back.
      config.onCommit(pendingValue)
    }

    target.addEventListener('pointermove', onMove)
    target.addEventListener('pointerup', cleanup)
    target.addEventListener('pointercancel', cleanup)
  }

  return { isDragging, begin }
}
