// Left-edge swipe-to-go-back gesture (Phase 3).
//
// Industry convergence (Ionic — our closest analog, plus react-navigation's JS
// stack and Flutter's Cupertino route) is a JS-driven gesture bound to the app's
// own back action, NOT WKWebView's `allowsBackForwardNavigationGestures` (which
// drives the wrong stack — webview history — with no custom transition and no
// hierarchical fallback). So we recognise the edge swipe ourselves and call the
// same intelligent `performBack` every other affordance uses.
//
// This is the threshold version: recognise the swipe and commit on release.
// Ionic's fully-interactive 1:1 finger-linked transition additionally needs the
// PREVIOUS view kept mounted behind the current one (a nav-stack / <keep-alive>
// concern) to avoid revealing a blank page mid-drag; that view retention is the
// planned polish follow-up. Params mirror Ionic's swipe-back.

import { onBeforeUnmount, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { canGoBackInApp, performBack } from '@/router/navigation'
import { useMobileDetection } from '@/composables/useMobileDetection'

/** Only a swipe starting within this many px of the leading edge counts. */
const EDGE_ZONE_PX = 50
/** Commit if dragged past this fraction of the viewport width... */
const COMMIT_FRACTION = 0.5
/** ...or flung faster than this (px per ms), like Ionic's velocity gate. */
const COMMIT_VELOCITY = 0.3
/** Ignore tiny drags entirely (a tap or jitter). */
const MIN_COMMIT_PX = 60

/**
 * Install the app-wide edge-swipe-back gesture. Call once from the root
 * component. No-op on non-mobile viewports and when there's nowhere to go back
 * to (so a swipe on a root screen does nothing, matching iOS).
 */
export function useSwipeBack(): void {
  const router = useRouter()
  const route = useRoute()
  const { isMobile } = useMobileDetection('sm')

  let startX = 0
  let startY = 0
  let startT = 0
  let tracking = false

  const canBack = () => canGoBackInApp.value || !!route.meta.parent

  function onStart(e: TouchEvent) {
    tracking = false
    if (!isMobile.value || !canBack() || e.touches.length !== 1) return
    const t = e.touches[0]
    if (t.clientX > EDGE_ZONE_PX) return // not from the leading edge
    startX = t.clientX
    startY = t.clientY
    startT = performance.now()
    tracking = true
  }

  function onMove(e: TouchEvent) {
    if (!tracking) return
    const t = e.touches[0]
    // Abandon if the gesture turns vertical — that's a scroll, not a back swipe.
    if (Math.abs(t.clientY - startY) > Math.abs(t.clientX - startX)) {
      tracking = false
    }
  }

  function onEnd(e: TouchEvent) {
    if (!tracking) return
    tracking = false
    const t = e.changedTouches[0]
    const dx = t.clientX - startX
    if (dx <= 0) return // must move rightward (leading -> trailing)
    const dt = performance.now() - startT
    const velocity = dt > 0 ? dx / dt : 0
    const committed =
      dx > window.innerWidth * COMMIT_FRACTION ||
      (dx > MIN_COMMIT_PX && velocity > COMMIT_VELOCITY)
    if (committed) performBack(router, route)
  }

  onMounted(() => {
    window.addEventListener('touchstart', onStart, { passive: true })
    window.addEventListener('touchmove', onMove, { passive: true })
    window.addEventListener('touchend', onEnd, { passive: true })
    window.addEventListener('touchcancel', () => (tracking = false), { passive: true })
  })
  onBeforeUnmount(() => {
    window.removeEventListener('touchstart', onStart)
    window.removeEventListener('touchmove', onMove)
    window.removeEventListener('touchend', onEnd)
  })
}
