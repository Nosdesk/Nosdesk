import type { IconName } from '@/components/common/icons';

export interface AdminNavItem {
  titleKey: string;
  descriptionKey: string;
  icon: IconName;
  route: string;
  keywords: string[];
  /** Visible to the standalone audit_reviewer role (Item C/D4). When
   * false/undefined the item is admin-only. */
  auditReviewerAllowed?: boolean;
  /** Visible only to platform admins (Nosdesk operators), not per-workspace
   * owners/admins. For cross-tenant operator tools. */
  platformAdminOnly?: boolean;
}

export interface AdminNavGroup {
  labelKey: string;
  items: AdminNavItem[];
}

export const adminNavGroups: AdminNavGroup[] = [
  {
    labelKey: 'admin-nav-group-tickets',
    items: [
      {
        titleKey: 'admin-nav-groups-title',
        descriptionKey: 'admin-nav-groups-description',
        icon: 'team',
        route: '/admin/groups',
        keywords: ['groups', 'teams', 'members', 'membership']
      },
      {
        titleKey: 'admin-nav-categories-title',
        descriptionKey: 'admin-nav-categories-description',
        icon: 'tag',
        route: '/admin/categories',
        keywords: ['categories', 'tags', 'ticket types', 'visibility']
      },
      {
        titleKey: 'admin-nav-user-fields-title',
        descriptionKey: 'admin-nav-user-fields-description',
        icon: 'account',
        route: '/admin/user-fields',
        keywords: ['user', 'fields', 'custom', 'contact', 'profile', 'properties', 'directory']
      },
      {
        titleKey: 'admin-nav-assignment-rules-title',
        descriptionKey: 'admin-nav-assignment-rules-description',
        icon: 'lightning',
        route: '/admin/assignment-rules',
        keywords: ['assignment', 'rules', 'auto-assign', 'routing', 'automation']
      },
      {
        titleKey: 'admin-nav-workflow-title',
        descriptionKey: 'admin-nav-workflow-description',
        icon: 'tag',
        route: '/admin/workflow',
        keywords: ['workflow', 'states', 'status', 'kanban', 'categories']
      },
      {
        titleKey: 'admin-nav-asset-kinds-title',
        descriptionKey: 'admin-nav-asset-kinds-description',
        icon: 'device',
        route: '/admin/asset-kinds',
        keywords: ['assets', 'kinds', 'devices', 'inventory', 'attributes', 'schema', 'discriminator', 'custom']
      },
      {
        titleKey: 'admin-nav-sla-title',
        descriptionKey: 'admin-nav-sla-description',
        icon: 'clock',
        route: '/admin/sla',
        keywords: ['sla', 'service level', 'response time', 'resolution time', 'breach', 'policies', 'calendars', 'working hours']
      },
      {
        titleKey: 'admin-nav-canned-responses-title',
        descriptionKey: 'admin-nav-canned-responses-description',
        icon: 'comment',
        route: '/admin/canned-responses',
        keywords: ['canned', 'responses', 'templates', 'replies', 'reusable', 'snippets', 'saved replies', 'macros']
      }
    ]
  },
  {
    labelKey: 'admin-nav-group-communication',
    items: [
      {
        titleKey: 'admin-nav-channels-title',
        descriptionKey: 'admin-nav-channels-description',
        icon: 'email',
        route: '/admin/channels',
        keywords: ['email', 'imap', 'ingestion', 'inbox', 'mailbox', 'channel', 'channels', 'pipeline', 'tickets', 'source']
      },
      {
        titleKey: 'admin-nav-email-delivery-title',
        descriptionKey: 'admin-nav-email-delivery-description',
        icon: 'email',
        route: '/admin/email',
        keywords: ['email', 'delivery', 'sending', 'smtp', 'domain', 'dkim', 'spf', 'dmarc', 'queue', 'suppressions', 'bounce', 'unsubscribe', 'outbound', 'from']
      },
      {
        titleKey: 'admin-nav-notification-defaults-title',
        descriptionKey: 'admin-nav-notification-defaults-description',
        icon: 'bell',
        route: '/admin/notification-defaults',
        keywords: ['notifications', 'defaults', 'preferences', 'channels', 'digest', 'push', 'locked', 'workspace', 'frequency']
      }
    ]
  },
  {
    labelKey: 'admin-nav-group-integrations',
    items: [
      {
        titleKey: 'admin-nav-api-tokens-title',
        descriptionKey: 'admin-nav-api-tokens-description',
        icon: 'key',
        route: '/admin/api-tokens',
        keywords: ['api', 'tokens', 'keys', 'programmatic', 'access']
      },
      {
        titleKey: 'admin-nav-webhooks-title',
        descriptionKey: 'admin-nav-webhooks-description',
        icon: 'link',
        route: '/admin/webhooks',
        keywords: ['webhooks', 'hooks', 'events', 'external', 'integrations']
      },
      {
        titleKey: 'admin-nav-plugins-title',
        descriptionKey: 'admin-nav-plugins-description',
        icon: 'puzzle',
        route: '/admin/plugins',
        keywords: ['plugins', 'extensions', 'addons', 'integrations']
      },
      {
        titleKey: 'admin-nav-data-import-title',
        descriptionKey: 'admin-nav-data-import-description',
        icon: 'database',
        route: '/admin/data-import',
        keywords: ['import', 'data', 'intune', 'csv', 'microsoft', 'graph', 'migration']
      },
      {
        titleKey: 'admin-nav-ldap-title',
        descriptionKey: 'admin-nav-ldap-description',
        icon: 'directory',
        route: '/admin/ldap',
        keywords: ['ldap', 'active directory', 'directory', 'ad', 'sync', 'sso', 'entra', 'openldap']
      },
    ]
  },
  {
    labelKey: 'admin-nav-group-compliance',
    items: [
      {
        titleKey: 'admin-nav-audit-log-title',
        descriptionKey: 'admin-nav-audit-log-description',
        icon: 'clock',
        route: '/admin/audit',
        keywords: ['audit', 'log', 'history', 'forensic', 'compliance', 'changes', 'who', 'when', 'events', 'auth'],
        auditReviewerAllowed: true
      }
    ]
  },
  {
    labelKey: 'admin-nav-group-appearance',
    items: [
      {
        titleKey: 'admin-nav-branding-title',
        descriptionKey: 'admin-nav-branding-description',
        icon: 'paint',
        route: '/admin/settings/branding',
        keywords: ['branding', 'logo', 'theme', 'appearance', 'colors', 'customization']
      },
    ]
  },
  {
    labelKey: 'admin-nav-group-system',
    items: [
      {
        titleKey: 'admin-nav-workspaces-title',
        descriptionKey: 'admin-nav-workspaces-description',
        icon: 'folder',
        route: '/admin/workspaces',
        keywords: ['workspaces', 'tenants', 'lifecycle', 'archive', 'members', 'multi-tenant']
      },
      {
        titleKey: 'admin-nav-guest-access-title',
        descriptionKey: 'admin-nav-guest-access-description',
        icon: 'user',
        route: '/admin/guest-access',
        keywords: ['guest', 'public', 'anonymous', 'ticket submission', 'access', 'self-service']
      },
      {
        titleKey: 'admin-nav-auth-providers-title',
        descriptionKey: 'admin-nav-auth-providers-description',
        icon: 'lock',
        route: '/admin/auth-providers',
        keywords: ['auth', 'authentication', 'sso', 'saml', 'oidc', 'oauth', 'microsoft', 'entra', 'ldap', 'login', 'providers']
      },
      {
        titleKey: 'admin-nav-search-title',
        descriptionKey: 'admin-nav-search-description',
        icon: 'search',
        route: '/admin/search',
        keywords: ['search', 'index', 'indexing', 'reindex', 'statistics']
      },
      {
        titleKey: 'admin-nav-system-settings-title',
        descriptionKey: 'admin-nav-system-settings-description',
        icon: 'settings',
        route: '/admin/system-settings',
        keywords: ['system', 'settings', 'storage', 'maintenance', 'cleanup', 'configuration']
      },
      {
        titleKey: 'admin-nav-backup-restore-title',
        descriptionKey: 'admin-nav-backup-restore-description',
        icon: 'archive',
        route: '/admin/backup-restore',
        keywords: ['backup', 'restore', 'export', 'import', 'data', 'recovery']
      },
      {
        titleKey: 'admin-nav-unrouted-inbound-title',
        descriptionKey: 'admin-nav-unrouted-inbound-description',
        icon: 'inbox',
        route: '/admin/inbound/unrouted',
        keywords: ['inbound', 'unrouted', 'dead letter', 'forwarding', 'email', 'token', 'misconfigured'],
        platformAdminOnly: true
      },
      {
        titleKey: 'admin-nav-bug-reports-title',
        descriptionKey: 'admin-nav-bug-reports-description',
        icon: 'warning',
        route: '/admin/bug-reports',
        keywords: ['bug', 'report', 'report a problem', 'feedback', 'issue', 'problem'],
        platformAdminOnly: true
      }
    ]
  }
];

/** Get all nav items as a flat array */
export const allAdminNavItems = adminNavGroups.flatMap(g => g.items);

/**
 * Restrict nav groups to what a role may see. Admins see everything except
 * platform-admin-only items (unless they're also a platform admin); the
 * standalone audit_reviewer role sees only items flagged
 * `auditReviewerAllowed`. Empty groups are dropped.
 */
export function filterAdminNavGroupsForRole(
  groups: AdminNavGroup[],
  opts: { isAdmin: boolean; isAuditReviewer: boolean; isPlatformAdmin: boolean },
): AdminNavGroup[] {
  if (opts.isAdmin) {
    if (opts.isPlatformAdmin) return groups;
    // A per-workspace admin who isn't a platform operator: hide cross-tenant
    // operator tools.
    return groups
      .map(group => ({ ...group, items: group.items.filter(i => !i.platformAdminOnly) }))
      .filter(group => group.items.length > 0);
  }
  if (opts.isAuditReviewer) {
    return groups
      .map(group => ({ ...group, items: group.items.filter(i => i.auditReviewerAllowed) }))
      .filter(group => group.items.length > 0);
  }
  return [];
}

/** Check if a given route path is active for a nav item (handles sub-routes) */
export function isAdminRouteActive(currentPath: string, itemRoute: string): boolean {
  if (currentPath === itemRoute) return true;
  if (currentPath.startsWith(itemRoute + '/')) return true;
  if (itemRoute === '/admin/groups' && /^\/admin\/groups\/[^/]+\/configure/.test(currentPath)) return true;
  return false;
}

/**
 * Filter nav groups by a search query. Matches the user-visible
 * translated title and description plus the locale-independent
 * `keywords` list, so a search for "logo" hits Branding regardless
 * of whether the active locale spells the title that way.
 */
export function filterAdminNavGroups(
  groups: AdminNavGroup[],
  query: string,
  translate: (key: string) => string,
): AdminNavGroup[] {
  const q = query.toLowerCase().trim();
  if (!q) return groups;

  return groups
    .map(group => ({
      ...group,
      items: group.items.filter(item =>
        translate(item.titleKey).toLowerCase().includes(q) ||
        translate(item.descriptionKey).toLowerCase().includes(q) ||
        item.keywords.some(k => k.toLowerCase().includes(q))
      )
    }))
    .filter(group => group.items.length > 0);
}
