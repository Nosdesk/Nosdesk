/**
 * Tenant-vs-platform classification of every admin surface: the source of truth
 * for who may reach each `/admin` page. `tenant` = any workspace admin
 * (self-serve); `platform` = Nosdesk operators only (cross-tenant lifecycle /
 * instance infrastructure), which must be hidden from tenant admins in the nav
 * (`platformAdminOnly`) and route-guarded (`platformAdminRequired`).
 *
 * Adding an admin nav item without classifying it here fails
 * `adminSurfaceManifest.spec.ts`, forcing a deliberate gate decision so a new
 * operator surface can never silently leak to tenant admins and dead-end them
 * with a 403. See docs/code-review/saas-principles-review-2026-08.md and
 * docs/architecture/workspace-function-tiers.md.
 */
export type AdminSurfaceTier = 'tenant' | 'platform';

export const ADMIN_SURFACE_TIER: Record<string, AdminSurfaceTier> = {
  // Tenant self-serve: running your own workspace.
  '/admin/groups': 'tenant',
  '/admin/categories': 'tenant',
  '/admin/user-fields': 'tenant',
  '/admin/assignment-rules': 'tenant',
  '/admin/workflow': 'tenant',
  '/admin/asset-kinds': 'tenant',
  '/admin/sla': 'tenant',
  '/admin/canned-responses': 'tenant',
  '/admin/channels': 'tenant',
  '/admin/email': 'tenant',
  '/admin/notification-defaults': 'tenant',
  '/admin/api-tokens': 'tenant',
  '/admin/webhooks': 'tenant',
  '/admin/plugins': 'tenant',
  '/admin/data-import': 'tenant',
  '/admin/ldap': 'tenant',
  '/admin/audit': 'tenant',
  '/admin/settings/branding': 'tenant',
  '/admin/guest-access': 'tenant',
  // The System-settings page is tenant-facing (workspace data export); its
  // instance-wide maintenance buttons are gated inside the view.
  '/admin/system-settings': 'tenant',

  // Platform operator: cross-tenant lifecycle and instance infrastructure.
  '/admin/workspaces': 'platform',
  '/admin/auth-providers': 'platform',
  '/admin/search': 'platform',
  '/admin/backup-restore': 'platform',
  '/admin/inbound/unrouted': 'platform',
  '/admin/bug-reports': 'platform',
};

/** True when the given admin route is platform-operator-only. */
export function isPlatformAdminSurface(route: string): boolean {
  return ADMIN_SURFACE_TIER[route] === 'platform';
}
