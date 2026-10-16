import { describe, it, expect } from 'vitest'
import { adminNavGroups, filterAdminNavGroupsForRole } from './adminNavData'
import { ADMIN_SURFACE_TIER } from './adminSurfaceManifest'

/**
 * The affordance -> gate guardrail. Every `/admin` nav item must be classified
 * in the manifest, and its `platformAdminOnly` nav flag must match that tier.
 * This is what would have caught the operator-surfaces-shown-to-tenant-admins
 * class: a new admin page with no classification, or a platform page missing its
 * flag, fails here instead of dead-ending a tenant admin with a 403 in
 * production.
 */
describe('admin surface gating manifest', () => {
  const navItems = adminNavGroups.flatMap((g) => g.items)

  it('classifies every admin nav route (a new surface must declare a tier)', () => {
    const unclassified = navItems.map((i) => i.route).filter((route) => !(route in ADMIN_SURFACE_TIER))
    expect(unclassified, `unclassified admin routes: ${unclassified.join(', ')}`).toEqual([])
  })

  /** The nav route a manifest route is, or sits under. */
  const navParent = (route: string) =>
    navItems.find((i) => i.route === route) ?? navItems.find((i) => route.startsWith(`${i.route}/`))

  it('has no stale manifest entries', () => {
    const stale = Object.keys(ADMIN_SURFACE_TIER).filter((route) => !navParent(route))
    expect(stale, `manifest routes with no nav item: ${stale.join(', ')}`).toEqual([])
  })

  it('gives a sub-page its nav page\'s tier', () => {
    for (const route of Object.keys(ADMIN_SURFACE_TIER)) {
      const parent = navParent(route)
      if (!parent || parent.route === route) continue
      expect(ADMIN_SURFACE_TIER[route], `${route} under ${parent.route}`).toBe(ADMIN_SURFACE_TIER[parent.route])
    }
    expect(ADMIN_SURFACE_TIER['/admin/data-import/matches']).toBe('tenant')
  })

  it('flags exactly the platform surfaces as platformAdminOnly', () => {
    for (const item of navItems) {
      const tier = ADMIN_SURFACE_TIER[item.route]
      expect(
        Boolean(item.platformAdminOnly),
        `${item.route}: platformAdminOnly=${Boolean(item.platformAdminOnly)} but manifest tier=${tier}`,
      ).toBe(tier === 'platform')
    }
  })
})

describe('selfHostedOnly nav items', () => {
  const opts = { isAdmin: true, isAuditReviewer: false, isPlatformAdmin: true };
  const routes = (isHosted: boolean) =>
    filterAdminNavGroupsForRole(adminNavGroups, { ...opts, isHosted })
      .flatMap(g => g.items.map(i => i.route));

  it('shows the license page to a self-hosted platform admin', () => {
    expect(routes(false)).toContain('/admin/license');
  });

  it('hides it on hosted, where Nosdesk licenses the instance', () => {
    expect(routes(true)).not.toContain('/admin/license');
    expect(routes(true)).toContain('/admin/workspaces');
  });
});
