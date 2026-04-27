/**
 * Reactive flag that turns true only after a condition has been
 * sustained for `delayMs` milliseconds. Returns to false
 * immediately when the condition flips off.
 *
 * Use case: "show a spinner only if the request takes longer
 * than 250ms." For fast requests the indicator never appears
 * (no flash); for slow ones it appears smoothly.
 *
 * This is the heart of the "no skeleton flash" pattern, kills
 * the perceptual cost of showing a loading state for an
 * operation that completes before the user could even register
 * it.
 */
import { onScopeDispose, ref, watch, type Ref } from 'vue'

export function useDelayedFlag(
  condition: () => boolean,
  delayMs = 300,
): Readonly<Ref<boolean>> {
  const flag = ref(false)
  let timer: ReturnType<typeof setTimeout> | null = null

  function clearTimer() {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  watch(
    condition,
    (active) => {
      clearTimer()
      if (active) {
        timer = setTimeout(() => {
          flag.value = true
          timer = null
        }, delayMs)
      } else {
        // Off is immediate. Keeping the off-transition gated by
        // delay would let stale spinners linger after the request
        // finished, exactly the bug we're fighting.
        flag.value = false
      }
    },
    { immediate: true },
  )

  onScopeDispose(clearTimer)

  return flag
}
