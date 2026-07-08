/**
 * Pull-to-refresh gesture for the Tauri mobile app.
 *
 * The app shell never scrolls the document — each view scrolls an
 * inner element — so this attaches to that scroll container and
 * owns the gesture end-to-end: Tauri's webview disables the iOS
 * root rubber-band (wry hardcodes `scrollView.bounces = false`)
 * and Android's WebView has no native pull-to-refresh, which means
 * touch events at `scrollTop === 0` arrive uncontested.
 *
 * State machine:
 *
 *   idle ─ touchstart at top ─→ (tracking, unclassified)
 *   tracking ─ downward + vertical-dominant past slop ─→ pulling
 *   tracking ─ upward / horizontal / scrolled ─→ idle (native wins)
 *   pulling ⇄ armed         (damped distance vs threshold; haptic
 *                            once per gesture on the upward crossing)
 *   pulling ─ release ─→ settling ─→ idle
 *   armed   ─ release ─→ refreshing ─→ settling ─→ idle
 *
 * Perf contract: the only permanent listener is a passive
 * `touchstart`. The non-passive `touchmove` (needed so a pull can
 * `preventDefault()` native scrolling) exists only between a
 * top-of-scroll touchstart and that gesture's end, and an
 * upward/horizontal gesture detaches it within ~10px of movement —
 * scrolling never pays for it. `preventDefault()` is never called
 * before the gesture is classified as a pull, so taps, long-presses
 * (pointer-event based) and the WKWebView horizontal swipe-back all
 * pass through untouched.
 *
 * The visual is iOS-style: the scroll container itself translates
 * down (paint-only — `scrollTop`, scroll restoration and
 * IntersectionObserver roots are untouched). Under
 * `prefers-reduced-motion` no transforms are written; the indicator
 * alone conveys the gesture.
 */
import {
  computed,
  getCurrentInstance,
  onActivated,
  onDeactivated,
  onScopeDispose,
  readonly,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { isTauriRuntime } from '@/platform'
import { hapticImpactLight } from '@/platform/haptics'
import { useReducedMotion } from '@/composables/useReducedMotion'

export type PullToRefreshState = 'idle' | 'pulling' | 'armed' | 'refreshing' | 'settling'

/** Fixed-position anchor of the scroller's top edge, captured at gesture start. */
export interface PullToRefreshAnchor {
  top: number
  left: number
  width: number
}

export interface UsePullToRefreshOptions {
  /** The scroll container: gates arming on its `scrollTop` and gets translated. */
  target: MaybeRefOrGetter<HTMLElement | null | undefined>
  /** Refresh work. Errors and timeouts are swallowed — the indicator
   * always resolves to the success visual (the data layer is live via
   * SSE regardless; the gesture is reassurance, not correctness). */
  onRefresh: () => Promise<unknown>
  /** When false nothing is attached at all. Defaults to the Tauri runtime check. */
  enabled?: MaybeRefOrGetter<boolean>
  /** Damped pull distance (px) that arms the refresh. */
  threshold?: number
  /** Damping asymptote — the pull can never exceed this. */
  maxPull?: number
  /** Offset the content holds at while refreshing. */
  holdDistance?: number
  /** Minimum time the spinner stays visible, so instant responses don't flash. */
  minDisplayMs?: number
  /** Give up waiting on `onRefresh` after this long (still settles as success). */
  timeoutMs?: number
}

export interface UsePullToRefreshReturn {
  state: Readonly<Ref<PullToRefreshState>>
  /** Damped pull distance in px, rAF-updated during the gesture. */
  pullDistance: Readonly<Ref<number>>
  /** min(1, pullDistance / threshold) — drives the indicator arc. */
  progress: ComputedRef<number>
  isActive: ComputedRef<boolean>
  anchor: Readonly<Ref<PullToRefreshAnchor | null>>
  /** Config echo for consumers (indicator gap sizing). */
  holdDistance: number
}

/** Movement (px) before a touch is classified as a pull vs a tap/scroll. */
const SLOP = 10

/** Elements whose touches never start a pull: form fields, and the
 * drag-handle composables that mark their targets `touch-none`. */
const IGNORE_SELECTOR =
  'input, textarea, select, [contenteditable="true"], .touch-none, [data-ptr-ignore]'

const SETTLE_MS = 200
const SETTLE_EASING = 'cubic-bezier(0.22, 1, 0.36, 1)'

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export function usePullToRefresh(options: UsePullToRefreshOptions): UsePullToRefreshReturn {
  const threshold = options.threshold ?? 72
  const maxPull = options.maxPull ?? 140
  const holdDistance = options.holdDistance ?? 56
  const minDisplayMs = options.minDisplayMs ?? 500
  const timeoutMs = options.timeoutMs ?? 8000
  const enabled = options.enabled ?? (() => isTauriRuntime())

  const reducedMotion = useReducedMotion()

  const state = ref<PullToRefreshState>('idle')
  const pullDistance = ref(0)
  const anchor = ref<PullToRefreshAnchor | null>(null)
  const progress = computed(() => Math.min(1, pullDistance.value / threshold))
  const isActive = computed(() => state.value !== 'idle')

  // The element the permanent touchstart listener is attached to.
  let attachedEl: HTMLElement | null = null
  // The element the in-flight gesture is bound to (survives `target`
  // changing identity mid-gesture).
  let gestureEl: HTMLElement | null = null

  let startX = 0
  let startY = 0
  let classified = false
  let hapticFired = false
  let rawDy = 0
  let rafId: number | null = null
  let settleTimer: number | null = null
  let prevOverscroll: string | null = null
  // Bumped by every reset; async work (refresh, settle) checks it
  // hasn't been superseded before touching state.
  let generation = 0

  function damp(raw: number): number {
    return maxPull * (1 - Math.exp(-raw / maxPull))
  }

  function applyTransform(px: number): void {
    if (!gestureEl) return
    if (reducedMotion.value) return // indicator-only under reduced motion
    gestureEl.style.transform = px > 0 ? `translate3d(0, ${px}px, 0)` : ''
  }

  function scheduleFrame(): void {
    if (rafId !== null) return
    rafId = requestAnimationFrame(() => {
      rafId = null
      if (state.value !== 'pulling' && state.value !== 'armed') return
      const d = damp(Math.max(0, rawDy))
      pullDistance.value = d
      applyTransform(d)
      if (d >= threshold && state.value === 'pulling') {
        state.value = 'armed'
        if (!hapticFired) {
          hapticFired = true
          hapticImpactLight()
        }
      } else if (d < threshold && state.value === 'armed') {
        state.value = 'pulling'
      }
    })
  }

  function clearSettleTimer(): void {
    if (settleTimer !== null) {
      window.clearTimeout(settleTimer)
      settleTimer = null
    }
  }

  /** Animate the content to `px`, then run `done` (unless superseded). */
  function animateTo(px: number, done: () => void): void {
    const el = gestureEl
    const gen = generation
    if (!el || reducedMotion.value) {
      applyTransform(px)
      pullDistance.value = px
      done()
      return
    }
    clearSettleTimer()
    const finish = (): void => {
      if (gen !== generation) return
      clearSettleTimer()
      el.removeEventListener('transitionend', onEnd)
      el.style.transition = ''
      done()
    }
    const onEnd = (e: TransitionEvent): void => {
      if (e.target === el && e.propertyName === 'transform') finish()
    }
    el.addEventListener('transitionend', onEnd)
    settleTimer = window.setTimeout(finish, SETTLE_MS + 50)
    el.style.transition = `transform ${SETTLE_MS}ms ${SETTLE_EASING}`
    pullDistance.value = px
    applyTransform(px)
    if (px === 0) el.style.transform = ''
  }

  function detachGestureListeners(): void {
    if (!gestureEl) return
    gestureEl.removeEventListener('touchmove', onTouchMove)
    gestureEl.removeEventListener('touchend', onTouchEnd)
    gestureEl.removeEventListener('touchcancel', onTouchCancel)
  }

  /** Hard reset: cancel everything, wipe inline styles, back to idle. */
  function reset(): void {
    generation++
    clearSettleTimer()
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    detachGestureListeners()
    if (gestureEl) {
      gestureEl.style.transform = ''
      gestureEl.style.transition = ''
      gestureEl.style.willChange = ''
    }
    gestureEl = null
    classified = false
    hapticFired = false
    rawDy = 0
    state.value = 'idle'
    pullDistance.value = 0
    anchor.value = null
  }

  function onTouchStart(e: TouchEvent): void {
    if (state.value !== 'idle') return
    if (e.touches.length !== 1) return
    const el = attachedEl
    if (!el || el.scrollTop >= 1) return
    const target = e.target as Element | null
    if (target?.closest(IGNORE_SELECTOR)) return

    const touch = e.touches[0]
    startX = touch.clientX
    startY = touch.clientY
    classified = false
    hapticFired = false
    rawDy = 0
    gestureEl = el
    const rect = el.getBoundingClientRect()
    anchor.value = { top: rect.top, left: rect.left, width: rect.width }
    // Non-passive so a classified pull can preventDefault native scroll.
    el.addEventListener('touchmove', onTouchMove, { passive: false })
    el.addEventListener('touchend', onTouchEnd)
    el.addEventListener('touchcancel', onTouchCancel)
  }

  function abortGesture(): void {
    detachGestureListeners()
    gestureEl = null
    anchor.value = null
  }

  function onTouchMove(e: TouchEvent): void {
    const el = gestureEl
    if (!el) return
    if (e.touches.length !== 1) {
      if (classified) reset()
      else abortGesture()
      return
    }
    const touch = e.touches[0]
    const dx = touch.clientX - startX
    const dy = touch.clientY - startY

    if (!classified) {
      if (Math.abs(dx) < SLOP && Math.abs(dy) < SLOP) return // could still be a tap
      // Horizontal (swipe-back), upward (scroll), or no longer at the
      // top: not a pull — hand the gesture back to the platform.
      if (Math.abs(dx) >= Math.abs(dy) || dy < 0 || el.scrollTop >= 1) {
        abortGesture()
        return
      }
      classified = true
      state.value = 'pulling'
      el.style.willChange = 'transform'
    }

    e.preventDefault()
    rawDy = Math.max(0, dy - SLOP)
    scheduleFrame()
  }

  function onTouchEnd(): void {
    if (!classified) {
      abortGesture()
      return
    }
    detachGestureListeners()
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
    if (state.value === 'armed') {
      startRefresh()
    } else {
      settle()
    }
  }

  function onTouchCancel(): void {
    if (classified) reset()
    else abortGesture()
  }

  function settle(): void {
    state.value = 'settling'
    animateTo(0, () => {
      if (gestureEl) gestureEl.style.willChange = ''
      gestureEl = null
      classified = false
      state.value = 'idle'
      pullDistance.value = 0
      anchor.value = null
    })
  }

  function startRefresh(): void {
    const gen = generation
    state.value = 'refreshing'
    animateTo(holdDistance, () => {})

    const work = (async () => {
      try {
        await Promise.race([options.onRefresh(), sleep(timeoutMs)])
      } catch (err) {
        // Reassurance gesture: never surface failure here — the SSE
        // stream / delta poll owns correctness.
        console.warn('[pull-to-refresh] refresh failed', err)
      }
    })()

    void Promise.all([work, sleep(minDisplayMs)]).then(() => {
      if (gen !== generation || state.value !== 'refreshing') return
      settle()
    })
  }

  // ── Attachment ────────────────────────────────────────────────

  function detachRoot(): void {
    if (!attachedEl) return
    attachedEl.removeEventListener('touchstart', onTouchStart)
    if (prevOverscroll !== null) {
      attachedEl.style.overscrollBehaviorY = prevOverscroll
      prevOverscroll = null
    }
    attachedEl = null
  }

  watch(
    () => (toValue(enabled) ? (toValue(options.target) ?? null) : null),
    (el) => {
      if (el === attachedEl) return
      reset()
      detachRoot()
      if (!el) return
      attachedEl = el
      // Suppress any residual WebKit overscroll on the inner scroller
      // so the pull is the only thing that moves.
      prevOverscroll = el.style.overscrollBehaviorY
      el.style.overscrollBehaviorY = 'contain'
      el.addEventListener('touchstart', onTouchStart, { passive: true })
    },
    { immediate: true, flush: 'post' },
  )

  onScopeDispose(() => {
    reset()
    detachRoot()
  })

  // KeepAlive'd views (tickets/assets lists): a deactivation mid-gesture
  // or mid-refresh must not leave a stuck transform behind. The element
  // and its listeners survive deactivation; state does not.
  if (getCurrentInstance()) {
    onDeactivated(() => reset())
    onActivated(() => {
      // Element identity can change across activations; re-evaluate.
      const el = toValue(enabled) ? (toValue(options.target) ?? null) : null
      if (el !== attachedEl) {
        detachRoot()
        if (el) {
          attachedEl = el
          prevOverscroll = el.style.overscrollBehaviorY
          el.style.overscrollBehaviorY = 'contain'
          el.addEventListener('touchstart', onTouchStart, { passive: true })
        }
      }
    })
  }

  return {
    state: readonly(state),
    pullDistance: readonly(pullDistance),
    progress,
    isActive,
    anchor: readonly(anchor),
    holdDistance,
  }
}
