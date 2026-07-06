/**
 * Shared `prefers-reduced-motion` signal. One matchMedia listener
 * for the app; consumers read the ref (or the scroll-behavior
 * helper) instead of each wiring their own media query. CSS-only
 * animation should keep using the `motion-safe:` variant; this is
 * for imperative motion (smooth scrolling, JS transitions).
 */
import { ref, type Ref } from 'vue'

const reduced = ref(false)
let installed = false

function ensureListener(): void {
  if (installed || typeof window === 'undefined' || !window.matchMedia) return
  installed = true
  const mq = window.matchMedia('(prefers-reduced-motion: reduce)')
  reduced.value = mq.matches
  mq.addEventListener('change', (e) => {
    reduced.value = e.matches
  })
}

export function useReducedMotion(): Ref<boolean> {
  ensureListener()
  return reduced
}

/** `'smooth'` unless the user asked for reduced motion. */
export function scrollBehavior(): ScrollBehavior {
  ensureListener()
  return reduced.value ? 'auto' : 'smooth'
}
