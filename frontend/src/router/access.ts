/**
 * Who may open a route, from its matched records' meta. The router's guard
 * asks this before every navigation, and anything that offers a link (a
 * dashboard widget's action) asks it before showing one, so a link is never
 * offered to someone the guard would send back to the dashboard.
 *
 * Pure: it takes the viewer's roles rather than reading the auth store, which
 * imports the router.
 */
import type { RouteMeta } from 'vue-router'

export interface RouteViewer {
  isAdmin: boolean
  isAuditReviewer: boolean
  isPlatformAdmin: boolean
  workspaceRole?: string | null
}

/** The auth store's roles, as the access check reads them. */
export function routeViewer(auth: {
  isAdmin: boolean
  isAuditReviewer: boolean
  isPlatformAdmin: boolean
  user?: { workspace_role?: string | null } | null
}): RouteViewer {
  return {
    isAdmin: auth.isAdmin,
    isAuditReviewer: auth.isAuditReviewer,
    isPlatformAdmin: auth.isPlatformAdmin,
    workspaceRole: auth.user?.workspace_role ?? null,
  }
}

/** True when any matched record restricts who may open the route. */
export function isGatedRoute(matched: readonly { meta: RouteMeta }[]): boolean {
  return matched.some(
    (r) => r.meta.adminRequired || r.meta.workspaceAdminRequired || r.meta.platformAdminRequired,
  )
}

export function canOpenRoute(matched: readonly { meta: RouteMeta }[], viewer: RouteViewer): boolean {
  const has = (key: keyof RouteMeta) => matched.some((r) => Boolean(r.meta[key]))

  // Admin pages: workspace and platform admins. The audit reviewer reaches
  // only the pages flagged for them (the audit feed).
  if (has('adminRequired') && !viewer.isAdmin && !(has('auditReviewerAllowed') && viewer.isAuditReviewer)) {
    return false
  }
  // Tenant self-serve pages: owners and admins of the current workspace only,
  // not a platform admin passing through.
  if (has('workspaceAdminRequired') && viewer.workspaceRole !== 'owner' && viewer.workspaceRole !== 'admin') {
    return false
  }
  // Operator pages: platform admins only.
  if (has('platformAdminRequired') && !viewer.isPlatformAdmin) return false
  return true
}
