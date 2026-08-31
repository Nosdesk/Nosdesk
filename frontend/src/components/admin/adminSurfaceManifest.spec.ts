import { describe, it, expect } from 'vitest'
import { adminNavGroups } from './adminNavData'
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

  it('has no stale manifest entries', () => {
    const navRoutes = new Set(navItems.map((i) => i.route))
    const stale = Object.keys(ADMIN_SURFACE_TIER).filter((route) => !navRoutes.has(route))
    expect(stale, `manifest routes with no nav item: ${stale.join(', ')}`).toEqual([])
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
