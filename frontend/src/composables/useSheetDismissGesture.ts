/**
 * useSheetDismissGesture: swipe-down-to-dismiss for the app's bottom
 * sheets, built to feel like a native iOS sheet.
 *
 * Owns the gesture end to end on an already-rendered sheet:
 *   - arming: a press on the handle (`data-sheet-handle`) always arms;
 *     a press on the body arms only when every scroller between the
 *     finger and the panel is at its top, so pulling a scrolled list
 *     back up never moves the sheet
 *   - classification after a 10px slop: mostly vertical and (from the
 *     body) downward is a sheet drag; anything else is handed back to
 *     the platform untouched, so horizontal drags, text selection, a
 *     wide table's scroll and the WKWebView back-swipe keep working
 *   - tracking: the panel follows the finger 1:1 downward with a
 *     rubber-band stretch upward, transform only and rAF-coalesced;
 *     the backdrop dims in step
 *   - release: dismiss past a quarter of the panel's height or on a
 *     downward fling, otherwise spring back; the dismiss glides on from
 *     where the finger left the sheet, then `onDismiss` runs the
 *     consumer's normal close
 *
 * Touch input is driven by touch events, not pointer events. WebKit
 * fires `pointercancel` the moment it decides a touch is a pan, before
 * our slop is spent, and only a non-passive `touchmove` can veto that
 * pan with `preventDefault()`. The two gestures already proven in the
 * app's WKWebView (`usePullToRefresh`, the search sheet) work this way.
 * The non-passive listener exists only between an arming press and the
 * end of that gesture, and nothing is prevented until the drag is
 * classified, so scrolling in the body never pays for it and taps reach
 * their targets. Mouse and pen use pointer events with pointer capture
 * taken at classification, late enough that a click is still delivered
 * to what was pressed.
 *
 * Reka's Presence keeps the sheet mounted until its `data-state=closed`
 * keyframe ends. After the gesture's own glide the panel and backdrop
 * get an inline `animation: none`, so Presence sees no animation and
 * unmounts at once: one close animation, not two.
 *
 * Does NOT own: the sheet's chrome, focus management, scroll lock or the
 * open/close state (`onDismiss` asks the consumer to close; the parent
 * owns `open`, as with a backdrop dismiss).
 */
import { onScopeDispose, readonly, ref, toValue, watch, type MaybeRefOrGetter, type Ref } from 'vue'
import { useReducedMotion } from '@/composables/useReducedMotion'

/** Marks the region a drag may start from regardless of scroll position. */
export const SHEET_HANDLE_ATTR = 'data-sheet-handle'
/** Opts an element (and its subtree) out: it owns its own drags. */
export const SHEET_NO_DRAG_ATTR = 'data-sheet-no-drag'
/** Set on the panel from the dismiss decision until it unmounts. */
export const SHEET_DISMISSING_ATTR = 'data-sheet-dismissing'

/** Movement (px) before a press is a drag rather than a tap. iOS's pan
 *  recognizer uses about the same, so taps and drags split where the
 *  platform splits them. */
export const SLOP = 10
/** Share of the panel's height the sheet must travel to dismiss on a
 *  slow drag. vaul's default; a touch above the search sheet's 20%
 *  because these sheets carry forms whose input a stray drag would lose. */
export const DISMISS_FRACTION = 0.25
/** Downward speed (px/ms) that dismisses at any distance. Shared with
 *  the search sheet so a fling feels the same across the app. */
export const FLING_VELOCITY = 0.4
/** How far the sheet can be stretched upward past rest. */
const UPWARD_STRETCH = 40
/** Velocity is measured over the most recent samples in this window, so
 *  a pause before release reads as zero and distance decides. */
const VELOCITY_WINDOW_MS = 100

const SETTLE_MS = 300
/** The iOS sheet curve, as the other sheets' keyframes use. */
const SETTLE_EASING = 'cubic-bezier(0.32, 0.72, 0, 1)'
/** Emphasized accelerate, so a fling's momentum carries off-screen. */
const GLIDE_EASING = 'cubic-bezier(0.3, 0, 0.8, 0.15)'
const GLIDE_MIN_MS = 150
const GLIDE_MAX_MS = 300
const GLIDE_DEFAULT_MS = 240
/** Extra travel past the panel's own height so its shadow leaves too. */
const GLIDE_OVERSHOOT_PX = 32

const COARSE_POINTER_QUERY = '(pointer: coarse)'
const DEFAULT_DESKTOP_QUERY = '(min-width: 768px)'

/** Elements whose presses never start a drag: fields select text or
 *  scrub a value on a vertical move, an `<audio controls>` is all
 *  control bar, and `data-sheet-no-drag` is the opt-out for custom
 *  controls. */
const IGNORE_SELECTOR = [
  'input',
  'textarea',
  'select',
  '[contenteditable]:not([contenteditable="false"])',
  '[role="slider"]',
  'audio[controls]',
  `[${SHEET_NO_DRAG_ATTR}]`,
].join(', ')

/** Height (px) of the strip along the bottom of a `<video controls>`
 *  left to the platform. iOS draws its scrubber and times there, inside
 *  a shadow root whose presses all target the `<video>`, and the
 *  scrubber follows any drag that starts on it, so the strip never arms.
 *  Above it is the picture, where a downward drag dismisses as it does
 *  in Photos; taps and sideways drags still reach the controls because
 *  the classifier hands them back. Generous on purpose: the bar is
 *  about 48px, and the picture loses little to the margin. */
export const VIDEO_CONTROLS_STRIP = 64

export type DragOrigin = 'handle' | 'body'
export type MoveClass = 'pending' | 'drag' | 'abort'

/** Where a press landed: the handle may always drag; the body only when
 *  nothing under the finger is scrolled. */
export function dragOriginOf(target: EventTarget | null, panel: HTMLElement): DragOrigin {
  const el = target instanceof Element ? target : null
  const handle = el?.closest(`[${SHEET_HANDLE_ATTR}]`)
  return handle && panel.contains(handle) ? 'handle' : 'body'
}

/** True when a press at `clientY` on `target` belongs to the element
 *  under it rather than to the sheet. */
export function isIgnoredTarget(target: EventTarget | null, panel: HTMLElement, clientY: number): boolean {
  if (!(target instanceof Element)) return false
  const hit = target.closest(IGNORE_SELECTOR)
  if (hit !== null && panel.contains(hit)) return true
  const video = target.closest('video[controls]')
  return video !== null && panel.contains(video) && clientY > video.getBoundingClientRect().bottom - VIDEO_CONTROLS_STRIP
}

/** True when any element between the press and the panel is scrolled
 *  down. `<= 0` rather than `=== 0`: iOS reports a negative scrollTop
 *  mid-bounce, which is still "at the top". */
export function scrolledBetween(target: EventTarget | null, panel: HTMLElement): boolean {
  for (let el = target as HTMLElement | null; el && el !== panel; el = el.parentElement) {
    if (el.scrollTop > 0) return true
  }
  return false
}

/** Decide what a move of (dx, dy) since the press means. Pending until
 *  the slop is spent; then horizontal movement is never ours, and
 *  upward movement from the body is a scroll. The handle may be pulled
 *  up (it stretches) because nothing else could want that drag. */
export function classifyMove(dx: number, dy: number, origin: DragOrigin): MoveClass {
  if (Math.abs(dx) < SLOP && Math.abs(dy) < SLOP) return 'pending'
  if (Math.abs(dx) >= Math.abs(dy)) return 'abort'
  if (dy < 0 && origin === 'body') return 'abort'
  return 'drag'
}

/** Panel offset for a finger displacement. The slop is spent deciding,
 *  so the sheet starts from rest rather than jumping 10px. Downward is
 *  1:1; upward is damped toward `UPWARD_STRETCH`. */
export function dragOffset(dy: number): number {
  if (dy >= 0) return Math.max(0, dy - SLOP)
  const up = Math.max(0, -dy - SLOP)
  if (up === 0) return 0
  return -UPWARD_STRETCH * (1 - Math.exp(-up / UPWARD_STRETCH))
}

/** A downward fling dismisses at any distance; a flick back up keeps
 *  the sheet however far it was dragged; between the two, distance
 *  decides. */
export function shouldDismiss(offset: number, panelHeight: number, velocity: number): boolean {
  if (velocity >= FLING_VELOCITY) return true
  if (velocity <= -FLING_VELOCITY) return false
  return offset >= panelHeight * DISMISS_FRACTION
}

/** Backdrop opacity for a panel offset: fully dimmed at rest, clear
 *  once the sheet has left the viewport. */
export function backdropOpacity(offset: number, panelHeight: number): number {
  if (offset <= 0 || panelHeight <= 0) return 1
  return Math.max(0, 1 - offset / panelHeight)
}

interface Sample {
  t: number
  y: number
}

/** px/ms over the samples inside the velocity window; positive is down. */
export function velocityOf(samples: readonly Sample[]): number {
  if (samples.length < 2) return 0
  const last = samples[samples.length - 1]
  const first = samples[0]
  const dt = last.t - first.t
  return dt > 0 ? (last.y - first.y) / dt : 0
}

function pushSample(samples: Sample[], t: number, y: number): void {
  samples.push({ t, y })
  while (samples.length > 1 && t - samples[0].t > VELOCITY_WINDOW_MS) samples.shift()
}

/** Glide duration from the remaining travel and release speed, clamped
 *  so a hard fling still reads and a slow release still finishes. */
export function glideDuration(remainingPx: number, velocity: number): number {
  if (velocity <= 0.1) return GLIDE_DEFAULT_MS
  return Math.min(GLIDE_MAX_MS, Math.max(GLIDE_MIN_MS, remainingPx / velocity))
}

/** Below the desktop breakpoint with a touch-first pointer. Read at the
 *  press rather than cached so a rotation or a window resize is honored
 *  without listeners. With a fine pointer the gesture stays off: a
 *  mouse drag in the body is text selection, and the close button is
 *  a click away. */
export function isSheetPresentation(desktopQuery = DEFAULT_DESKTOP_QUERY): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false
  return !window.matchMedia(desktopQuery).matches && window.matchMedia(COARSE_POINTER_QUERY).matches
}

export interface UseSheetDismissGestureOptions {
  /** The sheet element: receives the press listener and the transform. */
  panel: MaybeRefOrGetter<HTMLElement | null | undefined>
  /** Dimmed in proportion to the drag; optional. */
  backdrop?: MaybeRefOrGetter<HTMLElement | null | undefined>
  /** Asked to close once the sheet has left the screen. The consumer
   *  closes exactly as it would for a backdrop dismiss. */
  onDismiss: () => void
  /** Consumer gate on top of the presentation check (an alert dialog
   *  passes false, as it refuses backdrop dismiss). */
  enabled?: MaybeRefOrGetter<boolean>
  /** Media query for the non-sheet presentation. Tailwind's `md`. */
  desktopQuery?: string
}

export interface UseSheetDismissGestureReturn {
  /** True from classification to release. */
  isDragging: Readonly<Ref<boolean>>
  /** True from the dismiss decision until the panel unmounts. */
  isDismissing: Readonly<Ref<boolean>>
}

type Phase = 'idle' | 'armed' | 'dragging' | 'settling' | 'dismissing'

interface Gesture {
  origin: DragOrigin
  /** Touch is driven by touch events, everything else by pointer events. */
  mode: 'touch' | 'pointer'
  pointerId: number
  startX: number
  startY: number
  classified: boolean
  panelHeight: number
  lastDy: number
  samples: Sample[]
}

export function useSheetDismissGesture(options: UseSheetDismissGestureOptions): UseSheetDismissGestureReturn {
  const reducedMotion = useReducedMotion()
  const isDragging = ref(false)
  const isDismissing = ref(false)

  let el: HTMLElement | null = null
  let backdrop: HTMLElement | null = null
  let phase: Phase = 'idle'
  let gesture: Gesture | null = null
  let rafId: number | null = null
  let timer: number | null = null
  // Bumped by every reset so a settle or glide that has already been
  // superseded cannot touch state when its timer fires.
  let generation = 0

  function clearTimer(): void {
    if (timer !== null) {
      window.clearTimeout(timer)
      timer = null
    }
  }

  function cancelFrame(): void {
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
  }

  function clearStyles(): void {
    if (el) {
      el.style.transform = ''
      el.style.transition = ''
      el.style.willChange = ''
      el.style.animation = ''
      el.removeAttribute(SHEET_DISMISSING_ATTR)
    }
    if (backdrop) {
      backdrop.style.opacity = ''
      backdrop.style.transition = ''
      backdrop.style.willChange = ''
      backdrop.style.animation = ''
    }
  }

  function paint(offset: number, height: number): void {
    if (!el) return
    el.style.transform = offset === 0 ? '' : `translate3d(0, ${offset}px, 0)`
    if (backdrop) backdrop.style.opacity = String(backdropOpacity(offset, height))
  }

  function scheduleFrame(): void {
    if (rafId !== null) return
    rafId = requestAnimationFrame(() => {
      rafId = null
      if (phase !== 'dragging' || !gesture) return
      paint(dragOffset(gesture.lastDy), gesture.panelHeight)
    })
  }

  // ── Listener lifecycle ─────────────────────────────────────────

  function attachGestureListeners(mode: Gesture['mode']): void {
    if (!el) return
    if (mode === 'touch') {
      // Non-passive: a classified drag vetoes the browser's pan.
      el.addEventListener('touchmove', onTouchMove, { passive: false })
      el.addEventListener('touchend', onTouchEnd)
      el.addEventListener('touchcancel', onTouchCancel)
    } else {
      el.addEventListener('pointermove', onPointerMove)
      el.addEventListener('pointerup', onPointerUp)
      el.addEventListener('pointercancel', onPointerCancel)
    }
  }

  function detachGestureListeners(): void {
    if (!el) return
    el.removeEventListener('touchmove', onTouchMove)
    el.removeEventListener('touchend', onTouchEnd)
    el.removeEventListener('touchcancel', onTouchCancel)
    el.removeEventListener('pointermove', onPointerMove)
    el.removeEventListener('pointerup', onPointerUp)
    el.removeEventListener('pointercancel', onPointerCancel)
  }

  /** Hard reset: nothing in flight, no inline styles, back to idle. */
  function reset(): void {
    generation++
    clearTimer()
    cancelFrame()
    detachGestureListeners()
    clearStyles()
    gesture = null
    phase = 'idle'
    isDragging.value = false
    isDismissing.value = false
  }

  /** The press turned out not to be ours: let go without a trace. */
  function abortGesture(): void {
    detachGestureListeners()
    gesture = null
    phase = 'idle'
  }

  // ── Press ──────────────────────────────────────────────────────

  function onPointerDown(e: PointerEvent): void {
    if (phase !== 'idle' || !el) return
    if (!e.isPrimary || e.button !== 0) return
    if (!(toValue(options.enabled) ?? true)) return
    if (!isSheetPresentation(options.desktopQuery)) return
    if (isIgnoredTarget(e.target, el, e.clientY)) return
    // A selection in progress owns its drag handles.
    const selection = document.getSelection()
    if (selection && !selection.isCollapsed) return
    const origin = dragOriginOf(e.target, el)
    if (origin === 'body' && scrolledBetween(e.target, el)) return

    gesture = {
      origin,
      mode: e.pointerType === 'touch' ? 'touch' : 'pointer',
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      classified: false,
      panelHeight: 0,
      lastDy: 0,
      samples: [],
    }
    phase = 'armed'
    attachGestureListeners(gesture.mode)
  }

  // ── Move ───────────────────────────────────────────────────────

  /** Shared by both input paths. Returns true once the move belongs to
   *  the sheet, so the touch path knows to veto the pan. */
  function onMove(x: number, y: number, t: number): boolean {
    if (!gesture || !el) return false
    const dx = x - gesture.startX
    const dy = y - gesture.startY

    if (!gesture.classified) {
      const verdict = classifyMove(dx, dy, gesture.origin)
      if (verdict === 'pending') return false
      if (verdict === 'abort') {
        abortGesture()
        return false
      }
      gesture.classified = true
      // Zero in jsdom and in a sheet measured mid-layout; the viewport
      // is the honest upper bound.
      gesture.panelHeight = el.getBoundingClientRect().height || window.innerHeight
      phase = 'dragging'
      isDragging.value = true
      el.style.willChange = 'transform'
      if (backdrop) backdrop.style.willChange = 'opacity'
      if (gesture.mode === 'pointer') {
        // Throws for a pointer the browser has already cancelled.
        try {
          el.setPointerCapture?.(gesture.pointerId)
        } catch {
          // The move stream still reaches the panel while the pointer is over it.
        }
      }
    }

    gesture.lastDy = dy
    pushSample(gesture.samples, t, y)
    scheduleFrame()
    return true
  }

  function onPointerMove(e: PointerEvent): void {
    if (!gesture || e.pointerId !== gesture.pointerId) return
    onMove(e.clientX, e.clientY, performance.now())
  }

  function onTouchMove(e: TouchEvent): void {
    if (!gesture) return
    if (e.touches.length !== 1) {
      // A second finger: the gesture is no longer a sheet drag.
      if (gesture.classified) settle()
      else abortGesture()
      return
    }
    const touch = e.touches[0]
    // Not cancelable once the browser has committed to a scroll; Chrome
    // logs an intervention for a preventDefault there.
    if (onMove(touch.clientX, touch.clientY, performance.now()) && e.cancelable) e.preventDefault()
  }

  // ── Release ────────────────────────────────────────────────────

  function onRelease(): void {
    if (!gesture) return
    if (!gesture.classified) {
      abortGesture()
      return
    }
    detachGestureListeners()
    cancelFrame()
    const offset = dragOffset(gesture.lastDy)
    const velocity = velocityOf(gesture.samples)
    const height = gesture.panelHeight
    isDragging.value = false
    if (shouldDismiss(offset, height, velocity)) {
      dismiss(offset, height, velocity)
    } else {
      settle()
    }
  }

  function onPointerUp(e: PointerEvent): void {
    if (!gesture || e.pointerId !== gesture.pointerId) return
    onRelease()
  }

  function onTouchEnd(): void {
    onRelease()
  }

  /** The platform took the input (a system gesture, an incoming call):
   *  the user did not let go on purpose, so the sheet goes back. */
  function onPointerCancel(e: PointerEvent): void {
    if (!gesture || e.pointerId !== gesture.pointerId) return
    if (gesture.classified) settle()
    else abortGesture()
  }

  function onTouchCancel(): void {
    if (!gesture) return
    if (gesture.classified) settle()
    else abortGesture()
  }

  // ── Outcomes ───────────────────────────────────────────────────

  /** Run `done` when the panel's transform transition ends, or after
   *  `ms` plus a margin should the event never arrive (a tab in the
   *  background, jsdom). */
  function afterTransition(ms: number, done: () => void): void {
    const target = el
    const gen = generation
    if (!target) {
      done()
      return
    }
    const finish = (): void => {
      if (gen !== generation) return
      clearTimer()
      target.removeEventListener('transitionend', onEnd)
      done()
    }
    const onEnd = (e: TransitionEvent): void => {
      if (e.target === target && e.propertyName === 'transform') finish()
    }
    target.addEventListener('transitionend', onEnd)
    timer = window.setTimeout(finish, ms + 50)
  }

  /** Spring back to rest. */
  function settle(): void {
    detachGestureListeners()
    cancelFrame()
    gesture = null
    isDragging.value = false
    phase = 'settling'
    if (!el || reducedMotion.value) {
      clearStyles()
      phase = 'idle'
      return
    }
    el.style.transition = `transform ${SETTLE_MS}ms ${SETTLE_EASING}`
    el.style.transform = ''
    if (backdrop) {
      backdrop.style.transition = `opacity ${SETTLE_MS}ms ${SETTLE_EASING}`
      backdrop.style.opacity = ''
    }
    afterTransition(SETTLE_MS, () => {
      clearStyles()
      phase = 'idle'
    })
  }

  /** Glide off-screen from where the finger left the sheet, then ask
   *  the consumer to close. */
  function dismiss(offset: number, height: number, velocity: number): void {
    gesture = null
    phase = 'dismissing'
    isDismissing.value = true
    if (!el) {
      finishDismiss()
      return
    }
    el.setAttribute(SHEET_DISMISSING_ATTR, '')
    if (reducedMotion.value) {
      finishDismiss()
      return
    }
    const remaining = Math.max(1, height + GLIDE_OVERSHOOT_PX - offset)
    const duration = glideDuration(remaining, velocity)
    el.style.transition = `transform ${duration}ms ${GLIDE_EASING}`
    el.style.transform = `translate3d(0, calc(100% + ${GLIDE_OVERSHOOT_PX}px), 0)`
    if (backdrop) {
      backdrop.style.transition = `opacity ${Math.min(duration, 200)}ms ease-out`
      backdrop.style.opacity = '0'
    }
    afterTransition(duration, finishDismiss)
  }

  function finishDismiss(): void {
    const target = el
    const gen = generation
    // The sheet is already off-screen (or motion is reduced): Reka's
    // closed keyframes would replay the exit from the top, so silence
    // them and let Presence unmount at once.
    if (target) {
      target.style.willChange = ''
      target.style.animation = 'none'
    }
    if (backdrop) {
      backdrop.style.willChange = ''
      backdrop.style.animation = 'none'
    }
    options.onDismiss()
    // A consumer that keeps the sheet open (an unsaved-changes guard)
    // must not be left with an invisible dialog holding focus: if the
    // panel is still in the document after the close would have
    // unmounted it, bring it back.
    timer = window.setTimeout(() => {
      if (gen !== generation || !target?.isConnected) return
      timer = null
      isDismissing.value = false
      settle()
    }, 100)
  }

  // ── Attachment ─────────────────────────────────────────────────

  watch(
    () => [toValue(options.panel) ?? null, toValue(options.backdrop) ?? null] as const,
    ([panelEl, backdropEl]) => {
      if (panelEl === el && backdropEl === backdrop) return
      reset()
      el?.removeEventListener('pointerdown', onPointerDown)
      el = panelEl
      backdrop = backdropEl
      el?.addEventListener('pointerdown', onPointerDown)
    },
    { immediate: true, flush: 'post' },
  )

  onScopeDispose(() => {
    reset()
    el?.removeEventListener('pointerdown', onPointerDown)
    el = null
    backdrop = null
  })

  return {
    isDragging: readonly(isDragging),
    isDismissing: readonly(isDismissing),
  }
}
