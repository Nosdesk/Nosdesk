// Intelligent back-navigation model (Phase 1).
//
// The industry-convergent pattern (Ionic, react-navigation, Flutter, and the
// Apple/Material guidelines) is: keep an explicit sense of the in-app
// navigation stack rather than trusting raw browser history, and make "back"
// mean "pop the stack if there's a real previous in-app view, otherwise go to
// the declared hierarchical parent" (Ionic's `defaultHref`, Material's "Up").
//
// We don't reinvent vue-router's stack; we derive a single reliable signal from
// it. `window.history.length` is unusable (it counts pre-app entries, so a deep
// link / cold start already reports > 1 and `router.back()` escapes into
// login/redirect stubs or out of the app). Instead we track an IN-APP DEPTH as a
// delta over vue-router's `history.state.position`: +1 on a forward push, -1 on
// a back/pop, unchanged on a replace. Auth/redirect guards here redirect by
// RETURNING a location (replace-style), so the login/workspace chain never
// inflates the depth. Depth 0 == the entry point (nothing in-app to go back to).

import { computed, ref } from 'vue'
import {
  useRoute,
  useRouter,
  type RouteLocationNormalized,
  type Router,
} from 'vue-router'

/** Forward pushes deep past this session's entry point. 0 == at entry. */
const depth = ref(0)
/** Last observed `history.state.position`, for delta direction detection. */
let lastPosition: number | null = null

function currentPosition(): number {
  const state = window.history.state as { position?: number } | null
  return typeof state?.position === 'number' ? state.position : 0
}

/**
 * Hook the router so every settled navigation updates the in-app depth. Call
 * once, right after the router is created. `afterEach` fires only for completed
 * navigations, so aborted/redirected ones never reach here; a replace keeps the
 * same position and leaves depth untouched.
 */
export function installNavigationTracking(router: Router): void {
  router.afterEach(() => {
    const pos = currentPosition()
    if (lastPosition === null) {
      lastPosition = pos // entry baseline; depth stays 0
      return
    }
    if (pos > lastPosition) depth.value += 1
    else if (pos < lastPosition) depth.value = Math.max(0, depth.value - 1)
    lastPosition = pos
  })
}

/**
 * True when there is a real previous IN-APP view to return to — never the entry
 * point, a login/redirect stub, or a page outside the app. This replaces the
 * unreliable `window.history.length > 1` check.
 */
export const canGoBackInApp = computed(() => depth.value > 0)

/**
 * The hierarchical parent to fall back to when there is no in-app history
 * (deep link / cold start): `meta.parent` (a path string, or a function of the
 * route for dynamic parents), else derived by stripping the last path segment
 * (`/tickets/123` → `/tickets`), else `/`. Targets are workspace-slug agnostic —
 * `installWorkspaceGuard` re-prefixes the active slug on navigation — so a
 * declared `meta.parent` must be slug-free, while the derived fallback naturally
 * preserves whatever prefix the current path already carries.
 */
export function resolveBackTarget(route: RouteLocationNormalized): string {
  const parent = route.meta?.parent
  if (typeof parent === 'function') return parent(route)
  if (typeof parent === 'string') return parent
  const path = route.path.replace(/\/+$/, '')
  const idx = path.lastIndexOf('/')
  return idx > 0 ? path.slice(0, idx) : '/'
}

/**
 * The one back action every affordance (back arrow, edge-swipe, in-view back
 * button) should call: pop the in-app stack when possible, otherwise navigate to
 * the hierarchical parent. Standalone form for non-setup callers (e.g. the
 * gesture handler); `useBackNavigation` wraps it for components.
 */
export function performBack(
  router: Router,
  route: RouteLocationNormalized,
  fallback?: string,
): void {
  if (canGoBackInApp.value) {
    router.back()
  } else {
    router.push(fallback ?? resolveBackTarget(route))
  }
}

/**
 * Composable: a reactive `canGoBack` for rendering the affordance + `goBack`.
 * `goBack` accepts an optional per-view fallback that overrides the route's
 * `meta.parent` / derived parent (used only when there's no in-app history) —
 * the migration path for views that currently pass an explicit `fallbackRoute`.
 */
export function useBackNavigation(): {
  canGoBack: typeof canGoBackInApp
  goBack: (fallback?: string) => void
} {
  const router = useRouter()
  const route = useRoute()
  return {
    canGoBack: canGoBackInApp,
    goBack: (fallback?: string) => performBack(router, route, fallback),
  }
}
