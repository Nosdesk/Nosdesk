/**
 * Long-press gesture for touch-first contextual menus. Returns
 * pointer event listeners the consumer spreads onto the target
 * element via `v-on`. Designed for the "hold a row to reveal a
 * menu" pattern that desktop users hit via right-click and that
 * mobile users hit via... nothing, until now.
 *
 * Behaviour:
 *   - Fires `handler` if the user holds the primary pointer for
 *     `delay` ms without moving more than `moveThreshold` px.
 *   - Cancels on pointer up, pointer cancel, or movement past the
 *     threshold (the user is starting a scroll/drag, not a press).
 *   - Triggers a short haptic blip when the gesture fires, on
 *     devices that expose `navigator.vibrate`. iOS doesn't, so
 *     this is best-effort polish — the menu opening is the real
 *     feedback.
 *
 * The composable does NOT preventDefault on `pointerdown` because
 * doing so blocks scrolling. Instead it relies on the long-press
 * resolving before any synthetic mouse / contextmenu events fire,
 * and consumers should `@contextmenu.prevent` separately if they
 * want to suppress the OS context menu on long-press.
 */
import { onScopeDispose, ref } from 'vue'

interface UseLongPressOptions {
  /** Hold duration before the gesture fires. Defaults to 500ms,
   * matching the iOS / Android system long-press delay. */
  delay?: number
  /** Movement (in CSS pixels) past which we treat the gesture as
   * a scroll/drag and abort. Defaults to 8px, which is generous
   * enough to ignore accidental finger jitter without missing a
   * deliberate swipe. */
  moveThreshold?: number
}

export interface UseLongPressReturn {
  /** Bind via `v-on="longPress.listeners"`. */
  listeners: {
    pointerdown: (event: PointerEvent) => void
    pointerup: (event: PointerEvent) => void
    pointermove: (event: PointerEvent) => void
    pointercancel: () => void
    pointerleave: () => void
  }
  /** True while the user is actively pressing — handy for hover-
   * style highlight on the pressed row. */
  pressing: ReturnType<typeof ref<boolean>>
}

export function useLongPress(
  handler: (event: PointerEvent) => void,
  opts: UseLongPressOptions = {},
): UseLongPressReturn {
  const delay = opts.delay ?? 500
  const moveThreshold = opts.moveThreshold ?? 8

  let timer: number | null = null
  let startX = 0
  let startY = 0
  const pressing = ref(false)

  function cancel() {
    if (timer !== null) {
      window.clearTimeout(timer)
      timer = null
    }
    pressing.value = false
  }

  function pointerdown(event: PointerEvent) {
    // Only react to primary inputs. Touch always reports button=0,
    // pen typically does too. Right-clicks (button=2) on a desktop
    // mouse should keep going to the existing contextmenu handler.
    if (event.button !== 0) return
    cancel()
    startX = event.clientX
    startY = event.clientY
    pressing.value = true
    timer = window.setTimeout(() => {
      timer = null
      pressing.value = false
      // Best-effort haptic. No-op on iOS Safari, fires on Android
      // Chrome and most desktop browsers (which silently do nothing).
      if (typeof navigator !== 'undefined' && 'vibrate' in navigator) {
        try {
          navigator.vibrate(10)
        } catch {
          // Some platforms throw on user-gesture violations.
        }
      }
      handler(event)
    }, delay)
  }

  function pointerup() {
    cancel()
  }

  function pointermove(event: PointerEvent) {
    if (timer === null) return
    const dx = event.clientX - startX
    const dy = event.clientY - startY
    if (Math.hypot(dx, dy) > moveThreshold) cancel()
  }

  function pointercancel() {
    cancel()
  }

  function pointerleave() {
    cancel()
  }

  onScopeDispose(cancel)

  return {
    listeners: { pointerdown, pointerup, pointermove, pointercancel, pointerleave },
    pressing,
  }
}
