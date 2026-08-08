/**
 * Hold-to-drag on touch, shared by the kanban cards and the gantt bars.
 *
 * A horizontal swipe on a draggable is ambiguous: pan the surface, or pick the
 * thing up? Distance cannot tell them apart, and the browser resolves the
 * ambiguity first and in favour of panning, cancelling the drag. Measured on a
 * 390px touch viewport, a drag gesture on a kanban card scrolled the board from
 * 612 to 1204 and the card never moved.
 *
 * So touch presses activate on TIME, not distance: swipe to pan, hold to pick
 * up, as every touch board does.
 *
 * This is deliberately NOT solved with `touch-action: none` on the draggable.
 * Cards cover most of a column and bars most of a row, so that would leave a
 * phone user almost no surface to pan from. Instead the window-level
 * `touchmove` listener preventDefaults only once the hold has activated: until
 * then the browser pans normally, and after activation the scroll never starts,
 * so pointer events keep flowing.
 *
 * Both drag implementations consumed a verbatim copy of this policy, including
 * its constants. One copy, so it cannot drift.
 */

/** Hold before a touch press becomes a drag. */
export const LONG_PRESS_MS = 350
/** Movement during the hold that abandons it in favour of panning. */
export const TOUCH_SLOP_PX = 10

export interface TouchHold {
  /** True while the in-flight press came from a touch pointer. */
  readonly isTouch: boolean
  /**
   * Call on `pointerdown`. For a touch pointer this arms the hold timer and
   * attaches the scroll suppressor, and `onActivate` fires once the hold
   * completes. For mouse and pen it does nothing and returns false, leaving the
   * caller's existing distance-based promotion in charge.
   */
  begin(event: PointerEvent, onActivate: () => void): boolean
  /** True when movement during the hold means the user is panning, not
   *  dragging, and the press should be abandoned to the browser. */
  exceedsSlop(dx: number, dy?: number): boolean
  /** Cancel a pending hold without tearing down the listener. */
  cancelPending(): void
  /** Tear down: timer and listener. Safe to call repeatedly. */
  end(): void
}

/**
 * @param isDragging Read at `touchmove` time to decide whether to suppress the
 * scroll. Passed as a callback rather than a flag because the caller owns the
 * drag state and it flips after `begin`.
 */
export function createTouchHold(isDragging: () => boolean): TouchHold {
  let timer: ReturnType<typeof setTimeout> | null = null
  let listening = false
  let isTouch = false

  // Non-passive, or the `preventDefault` is ignored: Chrome makes window-level
  // `touchmove` listeners passive by default, which would silently make this a
  // no-op and hand every gesture back to the scroller.
  const onTouchMove = (event: TouchEvent): void => {
    if (isDragging() && event.cancelable) event.preventDefault()
  }

  function cancelPending(): void {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  function end(): void {
    cancelPending()
    if (listening) {
      window.removeEventListener('touchmove', onTouchMove)
      listening = false
    }
    isTouch = false
  }

  return {
    get isTouch() {
      return isTouch
    },
    begin(event, onActivate) {
      isTouch = event.pointerType === 'touch'
      if (!isTouch) return false
      window.addEventListener('touchmove', onTouchMove, { passive: false })
      listening = true
      timer = setTimeout(() => {
        timer = null
        onActivate()
      }, LONG_PRESS_MS)
      return true
    },
    exceedsSlop(dx, dy = 0) {
      return Math.hypot(dx, dy) > TOUCH_SLOP_PX
    },
    cancelPending,
    end,
  }
}
