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

import {
  computed,
  onUnmounted,
  ref,
  toValue,
  watchEffect,
  type MaybeRefOrGetter,
} from 'vue'
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

/**
 * Direction of the last settled navigation: `back` on a pop (position went
 * down), else `forward` (push/replace/initial). Read by scroll restoration to
 * decide restore-vs-top. Lifecycle-driven consumers read this at mount time,
 * after `afterEach` has already run for the navigation.
 */
export const navDirection = ref<'forward' | 'back'>('forward')

/**
 * vue-router's authoritative history position. Prefer the router's OWN history
 * state (`router.options.history.state`) over `window.history.state` — in some
 * webviews the latter isn't the object vue-router wrote. Falls back to window,
 * then 0.
 */
function currentPosition(router: Router): number {
  const rs = (router.options.history.state as { position?: number } | null)
    ?.position
  if (typeof rs === 'number') return rs
  const ws = (window.history.state as { position?: number } | null)?.position
  return typeof ws === 'number' ? ws : 0
}

/**
 * Hook the router so every settled navigation updates the in-app depth. Call
 * once, right after the router is created. `afterEach` fires only for completed
 * navigations, so aborted/redirected ones never reach here; a replace keeps the
 * same position and leaves depth untouched.
 */
export function installNavigationTracking(router: Router): void {
  router.afterEach(() => {
    const pos = currentPosition(router)
    if (lastPosition === null) {
      lastPosition = pos // entry baseline; depth stays 0
    } else {
      if (pos > lastPosition) {
        depth.value += 1
        navDirection.value = 'forward'
      } else if (pos < lastPosition) {
        depth.value = Math.max(0, depth.value - 1)
        navDirection.value = 'back'
      } else {
        navDirection.value = 'forward' // replace / same position: land at top
      }
      lastPosition = pos
    }
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
/**
 * A back target contributed by the view currently on screen.
 *
 * `meta.parent` covers hierarchies the route can express. Some cannot: a cycle
 * lives under its project, but the project id is not in `/cycles/:uuid`, so the
 * parent is only knowable once the cycle has loaded.
 *
 * Without this the mobile header has nothing to offer on a cold start (no
 * in-app history to pop, no `meta.parent`), so a deep link would render no back
 * affordance at all once the view stops drawing its own.
 */
const viewBackFallback = ref<string | undefined>(undefined)

/** Read-only view of the current view's back target, for the header. */
export const backFallback = computed(() => viewBackFallback.value)

/**
 * Declare this view's back target for as long as it is mounted. Reactive, so a
 * target that resolves after data loads is picked up when it arrives.
 */
export function useViewBackFallback(
  target: MaybeRefOrGetter<string | undefined>,
): void {
  watchEffect(() => {
    viewBackFallback.value = toValue(target)
  })
  onUnmounted(() => {
    viewBackFallback.value = undefined
  })
}

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
