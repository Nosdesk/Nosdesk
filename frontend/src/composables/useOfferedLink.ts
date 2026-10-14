import { useRouter, type RouteLocationRaw } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { canOpenRoute, routeViewer } from '@/router/access'
import { withActiveSlug } from '@/router/workspaceRouting'

/**
 * A link to offer the viewer, or null when they can't open it. The check is
 * the router guard's own (`canOpenRoute`), so nobody is shown a link that
 * would bounce them to the dashboard. The returned location carries the
 * active workspace slug in path mode, so the href is the page's real URL.
 */
export function useOfferedLink(): (to: RouteLocationRaw | null | undefined) => RouteLocationRaw | null {
  const router = useRouter()
  const auth = useAuthStore()
  return (to) => {
    if (!to) return null
    if (!router) return to
    const linked = withActiveSlug(router, to)
    return canOpenRoute(router.resolve(linked).matched, routeViewer(auth)) ? linked : null
  }
}
