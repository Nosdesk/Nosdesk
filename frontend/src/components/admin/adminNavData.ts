import { AdminIcons } from '@/components/admin/AdminIcons';

export interface AdminNavItem {
  title: string;
  description: string;
  icon: string;
  route: string;
  keywords: string[];
}

export interface AdminNavGroup {
  label: string;
  items: AdminNavItem[];
}

export const adminNavGroups: AdminNavGroup[] = [
  {
    label: 'Tickets & Workflow',
    items: [
      {
        title: 'Groups',
        description: 'Manage user groups and memberships',
        icon: 'users',
        route: '/admin/groups',
        keywords: ['groups', 'teams', 'members', 'membership']
      },
      {
        title: 'Categories',
        description: 'Configure ticket categories and group visibility',
        icon: 'tag',
        route: '/admin/categories',
        keywords: ['categories', 'tags', 'ticket types', 'visibility']
      },
      {
        title: 'Assignment Rules',
        description: 'Configure automatic ticket assignment based on rules',
        icon: 'lightning',
        route: '/admin/assignment-rules',
        keywords: ['assignment', 'rules', 'auto-assign', 'routing', 'automation']
      }
    ]
  },
  {
    label: 'Integrations',
    items: [
      {
        title: 'API Tokens',
        description: 'Manage API tokens for programmatic access',
        icon: 'key',
        route: '/admin/api-tokens',
        keywords: ['api', 'tokens', 'keys', 'programmatic', 'access']
      },
      {
        title: 'Webhooks',
        description: 'Configure webhooks to send events to external services',
        icon: 'link',
        route: '/admin/webhooks',
        keywords: ['webhooks', 'hooks', 'events', 'external', 'integrations']
      },
      {
        title: 'Plugins',
        description: 'Manage installed plugins and integrations',
        icon: 'puzzle',
        route: '/admin/plugins',
        keywords: ['plugins', 'extensions', 'addons', 'integrations']
      },
      {
        title: 'Data Import',
        description: 'Import data from Intune, CSV files, and other sources',
        icon: 'database',
        route: '/admin/data-import',
        keywords: ['import', 'data', 'intune', 'csv', 'microsoft', 'graph', 'migration']
      }
    ]
  },
  {
    label: 'Appearance & Notifications',
    items: [
      {
        title: 'Branding',
        description: 'Customize the appearance and branding of the application',
        icon: 'paint',
        route: '/admin/settings/branding',
        keywords: ['branding', 'logo', 'theme', 'appearance', 'colors', 'customization']
      },
      {
        title: 'Email Configuration',
        description: 'Configure SMTP settings and send test emails',
        icon: 'mail',
        route: '/admin/email-settings',
        keywords: ['email', 'smtp', 'mail', 'notifications', 'configuration']
      }
    ]
  },
  {
    label: 'System',
    items: [
      {
        title: 'Authentication Providers',
        description: 'Configure SSO, Microsoft Entra, and local authentication',
        icon: 'lock',
        route: '/admin/auth-providers',
        keywords: ['auth', 'authentication', 'sso', 'saml', 'oidc', 'oauth', 'microsoft', 'entra', 'ldap', 'login', 'providers']
      },
      {
        title: 'Search',
        description: 'Manage the search index and view indexing statistics',
        icon: 'search',
        route: '/admin/search',
        keywords: ['search', 'index', 'indexing', 'reindex', 'statistics']
      },
      {
        title: 'System Settings',
        description: 'Manage storage, cleanup stale files, and system maintenance',
        icon: 'cog',
        route: '/admin/system-settings',
        keywords: ['system', 'settings', 'storage', 'maintenance', 'cleanup', 'configuration']
      },
      {
        title: 'Backup & Restore',
        description: 'Export and restore system data and attachments',
        icon: 'archive',
        route: '/admin/backup-restore',
        keywords: ['backup', 'restore', 'export', 'import', 'data', 'recovery']
      }
    ]
  }
];

/** Get all nav items as a flat array */
export const allAdminNavItems = adminNavGroups.flatMap(g => g.items);

/** Check if a given route path is active for a nav item (handles sub-routes) */
export function isAdminRouteActive(currentPath: string, itemRoute: string): boolean {
  if (currentPath === itemRoute) return true;
  if (currentPath.startsWith(itemRoute + '/')) return true;
  if (itemRoute === '/admin/groups' && /^\/admin\/groups\/[^/]+\/configure/.test(currentPath)) return true;
  return false;
}

/** Get the SVG inner HTML for a nav icon */
export function getAdminIconHtml(iconName: string): string {
  return AdminIcons[iconName as keyof typeof AdminIcons] || AdminIcons.plus;
}

/** Filter nav groups by a search query (matches title, description, keywords) */
export function filterAdminNavGroups(groups: AdminNavGroup[], query: string): AdminNavGroup[] {
  const q = query.toLowerCase().trim();
  if (!q) return groups;

  return groups
    .map(group => ({
      ...group,
      items: group.items.filter(item =>
        item.title.toLowerCase().includes(q) ||
        item.description.toLowerCase().includes(q) ||
        item.keywords.some(k => k.toLowerCase().includes(q))
      )
    }))
    .filter(group => group.items.length > 0);
}
