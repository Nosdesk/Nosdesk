/**
 * Hover-intent controller for a single shared HoverCard instance.
 *
 * One controller serves many targets (e.g. every gantt bar): each
 * target reports enter/leave with its payload, and the controller
 * debounces intent so the card neither flickers on a drive-by nor
 * lags when moving between adjacent targets:
 *
 * - cold open waits `openDelay`;
 * - while the card is open (or within `warmMs` of closing), moving
 *   to another target retargets instantly;
 * - leaving starts a short `closeDelay` grace so the pointer can
 *   travel onto the card itself (WCAG 1.4.13 hoverable).
 *
 * Focus counts as hover (wire `onTargetEnter` to focusin too) so
 * keyboard users get the same preview.
 */
import { ref, shallowRef, type Ref, type ShallowRef } from 'vue'

export interface HoverCardController<T = unknown> {
  open: Ref<boolean>
  anchorEl: ShallowRef<HTMLElement | null>
  payload: Ref<T | null>
  onTargetEnter: (el: HTMLElement, payload: T) => void
  onTargetLeave: () => void
  /** Pointer entered / left the card surface itself. */
  onCardEnter: () => void
  onCardLeave: () => void
  /** Immediate close (Escape, click-through, drag start). */
  dismiss: () => void
}

export function useHoverCard<T = unknown>(
  options: { openDelay?: number; closeDelay?: number; warmMs?: number } = {},
): HoverCardController<T> {
  const openDelay = options.openDelay ?? 300
  const closeDelay = options.closeDelay ?? 100
  const warmMs = options.warmMs ?? 300

  const open = ref(false)
  const anchorEl = shallowRef<HTMLElement | null>(null)
  const payload = ref(null) as Ref<T | null>

  let openTimer: ReturnType<typeof setTimeout> | null = null
  let closeTimer: ReturnType<typeof setTimeout> | null = null
  let lastCloseAt = 0

  function clearTimers(): void {
    if (openTimer) clearTimeout(openTimer)
    if (closeTimer) clearTimeout(closeTimer)
    openTimer = null
    closeTimer = null
  }

  function show(el: HTMLElement, p: T): void {
    anchorEl.value = el
    payload.value = p
    open.value = true
  }

  function close(): void {
    if (!open.value) return
    open.value = false
    lastCloseAt = Date.now()
  }

  function onTargetEnter(el: HTMLElement, p: T): void {
    clearTimers()
    if (open.value || Date.now() - lastCloseAt < warmMs) {
      show(el, p)
      return
    }
    openTimer = setTimeout(() => {
      openTimer = null
      show(el, p)
    }, openDelay)
  }

  function onTargetLeave(): void {
    if (openTimer) {
      clearTimeout(openTimer)
      openTimer = null
    }
    if (!open.value) return
    if (closeTimer) clearTimeout(closeTimer)
    closeTimer = setTimeout(() => {
      closeTimer = null
      close()
    }, closeDelay)
  }

  function onCardEnter(): void {
    if (closeTimer) {
      clearTimeout(closeTimer)
      closeTimer = null
    }
  }

  function dismiss(): void {
    clearTimers()
    close()
  }

  return {
    open,
    anchorEl,
    payload,
    onTargetEnter,
    onTargetLeave,
    onCardEnter,
    onCardLeave: onTargetLeave,
    dismiss,
  }
}
