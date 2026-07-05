import { nextTick, type Directive } from 'vue'
import { navDirection } from '@/router/navigation'

/**
 * Restore an inner scroll container's position on BACK navigation, land at the
 * top on forward. The expected platform contract (browsers, native iOS/Android,
 * NN/g): returning to a list you scrolled should keep your place.
 *
 * Why here and not vue-router's `scrollBehavior`: that only reaches window
 * scroll, but our views scroll inside their own `overflow-auto` root, so
 * `savedPosition` is always the window's (0). We save/restore the container's
 * `scrollTop` ourselves, keyed by the route.
 *
 * Why lifecycle hooks and not popstate/`savedPosition`: the iOS WKWebView
 * swipe-back gesture doesn't reliably fire `popstate` (WebKit bug 248303), but
 * the routed view component always mounts, so mount/unmount fires regardless of
 * how the navigation was triggered.
 *
 * Usage: `v-scroll-restore="route.fullPath"` on the scroll container (App.vue
 * wires it on the routed view root). KeepAlive'd views retain their own live
 * `scrollTop` and never hit these hooks on nav, so they're unaffected.
 */

// Route fullPath -> last scrollTop. In-memory: enough for within-session
// back/forward; a hard reload legitimately starts fresh.
const positions = new Map<string, number>()

interface ScrollEl extends HTMLElement {
  _scrollRestoreKey?: string
}

/** Re-apply across a few frames so late layout (cache-first render settling,
 *  images loading in) can't clamp the restore short. */
function applyRestore(el: ScrollEl, top: number): void {
  el.scrollTop = top
  requestAnimationFrame(() => {
    el.scrollTop = top
    requestAnimationFrame(() => {
      el.scrollTop = top
    })
  })
}

export const vScrollRestore: Directive<ScrollEl, string> = {
  mounted(el, binding) {
    const key = binding.value
    el._scrollRestoreKey = key
    if (navDirection.value === 'back' && positions.has(key)) {
      void nextTick(() => applyRestore(el, positions.get(key) ?? 0))
    } else {
      el.scrollTop = 0
    }
  },
  beforeUnmount(el) {
    // Save under the route this element was showing (captured at mount), not the
    // current route — by unmount time the app has already navigated away.
    if (el._scrollRestoreKey !== undefined) {
      positions.set(el._scrollRestoreKey, el.scrollTop)
    }
  },
}
