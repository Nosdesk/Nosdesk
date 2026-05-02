import { createRouter, createWebHistory, type RouteLocationNormalized } from 'vue-router'
import DashboardView from '../views/DashboardView.vue'
import TicketView from '../views/TicketView.vue'
import LoginView from '../views/LoginView.vue'
import PasswordResetView from '../views/PasswordResetView.vue'
import OnboardingView from '../views/OnboardingView.vue'
import ErrorView from '../views/ErrorView.vue'
// Sync-engine views — pool-backed, optimistic, real-time. The
// legacy REST views were retired alongside their dispatchers
// once the sync runtime was the canonical implementation.
import TicketsListView from '@/sync/views/TicketsListView.vue'
import ProjectsView from '../views/ProjectsView.vue'
import ProjectDetailView from '../views/ProjectDetailView.vue'
import UserProfileView from '../views/UserProfileView.vue'
import DocumentationIndexView from '@/views/DocumentationIndexView.vue'
import DocumentView from '@/views/DocumentView.vue'
import ProfileSettingsView from '@/views/ProfileSettingsView.vue'
import PDFViewerView from '@/views/PDFViewerView.vue'
import authService from '@/services/authService'
import { useInboxLoader } from '@/loaders/inboxLoader'
import { useTicketsListLoader } from '@/loaders/ticketsListLoader'
import type { Page, Article } from '@/services/documentationService'

declare module 'vue-router' {
  interface RouteMeta {
    /// Optional because children of an authenticated parent inherit
    /// `requiresAuth` via `to.matched.some(...)`. Set explicitly only
    /// to override the inherited value (e.g. a public child of a
    /// protected layout).
    requiresAuth?: boolean;
    title?: string;
    layout?: string;
    adminRequired?: boolean;
    createButtonText?: string;
    createButtonIcon?: 'plus' | 'ticket' | 'user' | 'device' | 'project' | 'document';
    preloadedDocument?: unknown;
  }
}

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: LoginView,
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Sign In'
      }
    },
    {
      path: '/reset-password',
      name: 'reset-password',
      component: PasswordResetView,
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Reset Password'
      }
    },
    {
      path: '/mfa-setup',
      name: 'mfa-setup',
      component: () => import('@/views/MFASetupView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'MFA Setup Required'
      }
    },
    {
      path: '/accept-invitation',
      name: 'accept-invitation',
      component: () => import('@/views/AcceptInvitationView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Accept Invitation'
      }
    },
    {
      path: '/onboarding',
      name: 'onboarding',
      component: OnboardingView,
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Setup - Nosdesk'
      }
    },
    {
      path: '/submit-ticket',
      name: 'guest-submit-ticket',
      component: () => import('@/views/public/GuestTicketSubmitView.vue'),
      meta: { layout: 'blank', requiresAuth: false, title: 'Submit a Ticket' }
    },
    {
      path: '/ticket-status/:token',
      name: 'guest-ticket-status',
      component: () => import('@/views/public/GuestTicketStatusView.vue'),
      props: true,
      meta: { layout: 'blank', requiresAuth: false, title: 'Ticket Status' }
    },
    {
      path: '/docs',
      name: 'public-docs-list',
      component: () => import('@/views/public/PublicDocsView.vue'),
      meta: { layout: 'blank', requiresAuth: false, title: 'Documentation' }
    },
    {
      path: '/docs/:slug',
      name: 'public-doc',
      component: () => import('@/views/public/PublicDocView.vue'),
      props: true,
      meta: { layout: 'blank', requiresAuth: false, title: 'Documentation' }
    },
    {
      path: '/help',
      name: 'public-help',
      component: () => import('@/views/public/HelpView.vue'),
      meta: { layout: 'blank', requiresAuth: false, title: 'Help' }
    },
    {
      path: '/',
      name: 'home',
      component: DashboardView,
      meta: {
        requiresAuth: true,
        title: 'Dashboard',
        createButtonText: 'Create Ticket',
        createButtonIcon: 'ticket',
      }
    },
    {
      path: '/inbox',
      name: 'inbox',
      component: () => import('@/views/NotificationInboxView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Inbox',
        // Data Loader. Runs DURING navigation, before the
        // component code is evaluated. Primes Pinia Colada's
        // notification list + unread count caches so the view
        // mounts with data already present (render-as-you-fetch).
        // The loader is eagerly imported so the
        // `DataLoaderPlugin` finds it when the route record is
        // matched; the cost is small (a few imports).
        loaders: [useInboxLoader],
      }
    },
    {
      path: '/tickets',
      name: 'tickets',
      component: TicketsListView,
      meta: {
        requiresAuth: true,
        title: 'Tickets',
        createButtonText: 'Create Ticket',
        createButtonIcon: 'ticket',
        // Data Loader: pre-fetches the first page during
        // navigation so the view mounts with data ready.
        loaders: [useTicketsListLoader],
      }
    },
    {
      path: '/tickets/:id',
      name: 'ticket-view',
      component: TicketView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'View Ticket',
        createButtonText: 'Create Ticket',
        createButtonIcon: 'ticket',
      },
      beforeEnter: (to) => {
        to.meta.key = to.params.id
      }
    },
    {
      path: '/users/:uuid',
      name: 'user-profile',
      component: UserProfileView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'User Profile'
      },
      beforeEnter: (to) => {
        // Set a generic title initially, the component will update it after fetching the user
        to.meta.title = 'User Profile'
      }
    },
    {
      path: '/users/:uuid/settings/:section?',
      name: 'user-settings',
      component: ProfileSettingsView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'User Settings',
        adminRequired: true
      },
      beforeEnter: (to) => {
        // Update title based on section
        const section = to.params.section as string;
        const sectionTitles: Record<string, string> = {
          profile: 'User Profile Settings',
          appearance: 'User Appearance Settings',
          notifications: 'User Notification Settings',
          security: 'User Security Settings'
        };
        
        if (section && sectionTitles[section]) {
          to.meta.title = sectionTitles[section];
        } else {
          // No section param means base settings URL = profile section
          to.meta.title = 'User Profile Settings';
        }
      }
    },
    {
      path: '/projects',
      name: 'projects',
      component: ProjectsView,
      meta: {
        requiresAuth: true,
        title: 'Projects',
        createButtonText: 'Create Project',
        createButtonIcon: 'project',
      }
    },
    {
      path: '/projects/:id',
      name: 'project-detail',
      component: ProjectDetailView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Project Details',
        createButtonText: 'Add Ticket',
        createButtonIcon: 'ticket',
      },
      beforeEnter: (to) => {
        to.meta.key = to.params.id
      }
    },
    {
      path: '/error/:code?/:message?',
      name: 'error',
      component: ErrorView,
      props: true,
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Error'
      }
    },
    {
      path: '/users',
      name: 'users',
      component: () => import('../views/UsersListView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Users',
        createButtonText: 'Create User',
        createButtonIcon: 'user',
      }
    },
    {
      path: '/devices',
      name: 'devices',
      component: () => import('../views/DevicesListView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Devices',
        createButtonText: 'Create Device',
        createButtonIcon: 'device',
      }
    },
    {
      path: '/devices/new',
      name: 'device-create',
      component: () => import('../views/DeviceView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Create Device'
      }
    },
    {
      path: '/devices/:id',
      name: 'device-view',
      component: () => import('../views/DeviceView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Device Details'
      }
    },
    {
      path: '/documentation',
      name: 'documentation',
      component: DocumentationIndexView,
      meta: {
        requiresAuth: true,
        title: 'Documentation',
        createButtonText: 'Create Document',
        createButtonIcon: 'document',
      }
    },
    {
      path: '/documentation/drafts',
      name: 'documentation-drafts',
      component: () => import('../views/DocumentationDraftsView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Drafts'
      }
    },
    {
      path: '/documentation/collections/:slug',
      name: 'collection-view',
      component: () => import('../views/CollectionView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Collection'
      }
    },
    {
      path: '/documentation/archived',
      name: 'documentation-archived',
      component: () => import('../views/DocumentationArchivedView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Archived'
      }
    },
    {
      path: '/documentation/trash',
      name: 'documentation-trash',
      component: () => import('../views/DocumentationTrashView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Trash'
      }
    },
    {
      path: '/documentation/gaps',
      name: 'documentation-gaps',
      component: () => import('../views/DocumentationGapsView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Knowledge Gaps'
      }
    },
    {
      path: '/documentation/gaps/:id',
      name: 'documentation-gap-detail',
      component: () => import('../views/DocumentationGapsView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Knowledge Gaps'
      }
    },
    {
      path: '/documentation/:path',
      name: 'documentation-page',
      component: DocumentView,
      meta: {
        requiresAuth: true,
        title: 'Documentation'
      },
      props: true,
      beforeEnter: async (to) => {
        // Set a generic title initially
        to.meta.title = 'Documentation';
        to.meta.preloadedDocument = undefined;

        // Handle ticket notes — preloaded inside component (needs different data shape)
        if (to.query.ticketId) {
          to.meta.title = `Ticket #${to.query.ticketId} Notes`;
          return;
        }

        // Preload document data so the view renders instantly
        const path = to.params.path?.toString();
        if (path) {
          try {
            const { getPageByPath, getArticleById } = await import('@/services/documentationService');
            const result = await getPageByPath(path);

            if (result) {
              let doc: Page | Article = result;
              // If the result is a page stub (no children array), fetch full article
              if (!('children' in result && Array.isArray(result.children)) && 'id' in result) {
                const articleData = await getArticleById(String(result.id));
                if (articleData) doc = articleData;
                else return '/documentation'; // not found — redirect
              }
              to.meta.preloadedDocument = doc;
              to.meta.title = doc.title || 'Documentation';
            } else {
              return '/documentation'; // not found — redirect
            }
          } catch {
            return '/documentation';
          }
        }
      }
    },
    {
      path: '/profile',
      name: 'profile-redirect',
      component: () => null, // Empty component since this route redirects
      meta: {
        requiresAuth: true,
        title: 'Profile'
      },
      beforeEnter: async (to, from, next) => {
        // Import auth store to get current user
        const { useAuthStore } = await import('@/stores/auth');
        const authStore = useAuthStore();
        
        // If user is authenticated and has a UUID, redirect to their profile
        if (authStore.user?.uuid) {
          next(`/users/${authStore.user.uuid}`);
        } else {
          // Fallback to profile settings if no user UUID is available
          next('/profile/settings');
        }
      }
    },
    {
      path: '/profile/settings/:section?',
      name: 'profile-settings',
      component: ProfileSettingsView,
      meta: {
        requiresAuth: true,
        title: 'Settings'
      },
      beforeEnter: (to) => {
        // Update title based on section
        const section = to.params.section as string;
        const sectionTitles: Record<string, string> = {
          profile: 'Profile Settings',
          appearance: 'Appearance Settings',
          notifications: 'Notification Settings',
          security: 'Security Settings'
        };
        
        if (section && sectionTitles[section]) {
          to.meta.title = sectionTitles[section];
        } else {
          // No section param means base /profile/settings URL = profile section
          to.meta.title = 'Profile Settings';
        }
      }
    },
    {
      path: '/admin',
      component: () => import('../views/AdminLayout.vue'),
      meta: {
        requiresAuth: true,
        adminRequired: true
      },
      children: [
        {
          path: '',
          name: 'admin-index',
          component: () => import('../views/AdminIndexView.vue'),
          meta: { title: 'Administration' }
        },
        { path: 'settings', redirect: '/admin' },
        {
          path: 'groups',
          name: 'admin-groups',
          component: () => import('../views/GroupsManagementView.vue'),
          meta: { title: 'Groups' }
        },
        {
          path: 'groups/:uuid/configure',
          name: 'group-configuration',
          component: () => import('../views/GroupConfigurationView.vue'),
          props: true,
          meta: { title: 'Group Configuration' }
        },
        {
          path: 'categories',
          name: 'admin-categories',
          component: () => import('../views/CategoriesManagementView.vue'),
          meta: { title: 'Categories' }
        },
        {
          path: 'assignment-rules',
          name: 'admin-assignment-rules',
          component: () => import('../views/AssignmentRulesView.vue'),
          meta: { title: 'Assignment Rules' }
        },
        {
          path: 'workflow',
          name: 'admin-workflow',
          component: () => import('../views/admin/WorkflowStatesView.vue'),
          meta: { title: 'Workflow' }
        },
        {
          path: 'api-tokens',
          name: 'admin-api-tokens',
          component: () => import('../views/ApiTokensView.vue'),
          meta: { title: 'API Tokens' }
        },
        {
          path: 'webhooks',
          name: 'admin-webhooks',
          component: () => import('../views/WebhooksView.vue'),
          meta: { title: 'Webhooks' }
        },
        {
          path: 'plugins',
          name: 'admin-plugins',
          component: () => import('../views/admin/plugins/PluginListView.vue'),
          meta: { title: 'Plugins' }
        },
        {
          path: 'plugins/registry',
          name: 'admin-plugin-registry',
          component: () => import('../views/PluginRegistryView.vue'),
          meta: { title: 'Plugin Registry' }
        },
        {
          path: 'plugins/install',
          name: 'admin-plugin-sideload',
          component: () => import('../views/admin/plugins/PluginSideloadView.vue'),
          meta: { title: 'Sideload Plugin' }
        },
        {
          path: 'plugins/:uuid',
          name: 'admin-plugin-detail',
          component: () => import('../views/admin/plugins/PluginDetailView.vue'),
          meta: { title: 'Plugin Detail' }
        },
        {
          path: 'auth-providers',
          name: 'admin-auth-providers',
          component: () => import('../views/AuthProvidersView.vue'),
          meta: { title: 'Authentication Providers' }
        },
        {
          path: 'search',
          name: 'admin-search',
          component: () => import('../views/SearchManagementView.vue'),
          meta: { title: 'Search Index Management' }
        },
        {
          path: 'system-settings',
          name: 'admin-system-settings',
          component: () => import('../views/SystemSettingsView.vue'),
          meta: { title: 'System Settings' }
        },
        {
          path: 'settings/branding',
          name: 'admin-branding',
          component: () => import('../views/BrandingSettingsView.vue'),
          meta: { title: 'Branding' }
        },
        {
          path: 'guest-access',
          name: 'admin-guest-access',
          component: () => import('../views/GuestAccessSettingsView.vue'),
          meta: { title: 'Guest Access', requiresAuth: true, adminRequired: true }
        },
        {
          path: 'email-settings',
          name: 'admin-email-settings',
          component: () => import('../views/EmailSettingsView.vue'),
          meta: { title: 'Email Configuration' }
        },
        {
          path: 'channels/email',
          name: 'admin-channels-email',
          component: () => import('../views/ChannelsEmailSettingsView.vue'),
          meta: { title: 'Email Ingestion', requiresAuth: true, adminRequired: true }
        },
        {
          path: 'data-import',
          name: 'admin-data-import',
          component: () => import('../views/DataImportView.vue'),
          meta: { title: 'Data Import' }
        },
        {
          path: 'data-import/microsoft-graph',
          name: 'admin-microsoft-graph',
          component: () => import('../views/MicrosoftGraphView.vue'),
          meta: { title: 'Microsoft Graph Connection' }
        },
        {
          path: 'data-import/csv',
          name: 'admin-csv-import',
          component: () => import('../views/CsvImportView.vue'),
          meta: { title: 'CSV Import' }
        },
        {
          path: 'backup-restore',
          name: 'admin-backup-restore',
          component: () => import('../views/BackupRestoreView.vue'),
          meta: { title: 'Backup & Restore' }
        }
      ]
    },
    {
      path: '/groups/:uuid',
      name: 'group-detail',
      component: () => import('../views/GroupDetailView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Group Details'
      }
    },
    {
      path: '/auth/microsoft/callback',
      name: 'microsoft-callback',
      component: () => import('../views/auth/OAuthCallbackView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Authenticating...',
        oauthProvider: 'microsoft'
      }
    },
    {
      path: '/auth/oidc/callback',
      name: 'oidc-callback',
      component: () => import('../views/auth/OAuthCallbackView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Authenticating...',
        oauthProvider: 'oidc'
      }
    },
    {
      path: '/pdf-viewer',
      name: 'pdf-viewer',
      component: PDFViewerView,
      meta: {
        requiresAuth: true,
        title: 'PDF Viewer',
        titleIcon: 'pdf'
      }
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/error/404'
    }
  ],
})

// Update document title on navigation
// Routes where useTitleManager handles document.title (skip generic title-setting)
const titleManagedRoutes = ['ticket', 'device', 'documentation-article'];

router.beforeResolve((to) => {
  // Skip title-setting for routes managed by useTitleManager —
  // those views set their own specific title (e.g. "#123 Fix the bug")
  if (titleManagedRoutes.includes(to.name as string)) {
    return;
  }

  let title: string;

  if (to.meta?.title) {
    title = to.meta.title as string;
  } else if (to.name) {
    title = to.name.toString()
      .split('-')
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  } else {
    title = 'Nosdesk';
  }

  document.title = `${title} | Nosdesk`;
});

// ===== NAVIGATION GUARD MIDDLEWARE =====
// Modern Vue Router 4 pattern using return values instead of next() callbacks

/**
 * Check if system requires initial setup/onboarding
 * Redirects to onboarding if no admin user exists
 * Result is cached after first successful check to avoid API calls on every navigation
 */
let onboardingChecked = false;

async function checkOnboarding(to: RouteLocationNormalized, _from: RouteLocationNormalized) {
  // Skip check for onboarding, error, and login pages
  if (to.name === 'onboarding' || to.name === 'error' || to.name === 'login') {
    return;
  }

  // Once setup is confirmed complete, no need to re-check
  if (onboardingChecked) {
    return;
  }

  try {
    const setupStatus = await authService.checkSetupStatus();

    if (setupStatus.requires_setup) {
      return { name: 'onboarding' };
    }

    // Setup is complete — cache the result
    onboardingChecked = true;
  } catch (error) {
    console.error('Failed to check setup status:', error);
    // Continue navigation - error handled by onboarding component if needed
  }
}

/**
 * Fetch user data if authenticated but not yet loaded
 * Handles authentication state and redirects
 */
async function checkAuthentication(to: RouteLocationNormalized, _from: RouteLocationNormalized) {
  const { useAuthStore } = await import('@/stores/auth');
  const authStore = useAuthStore();

  const requiresAuth = to.matched.some((record) => record.meta.requiresAuth);

  // Fetch user data if authenticated (cookie/user present) but user object not yet loaded
  if (authStore.isAuthenticated && !authStore.user && !authStore.loading && to.name !== 'login') {
    try {
      await authStore.fetchUserData();
    } catch {
      // fetchUserData handles errors internally; the apiConfig interceptor
      // already handles 401 → refresh → retry. If it still fails, redirect.
      if (requiresAuth) {
        return { name: 'login', query: { redirect: to.fullPath } };
      }
    }
  }

  // Session restoration: when frontend auth state is cleared (e.g. page refresh after
  // the 15-min access token expires), attempt to restore the session using the 7-day
  // refresh token. fetchUserData() calls /auth/me → gets 401 → apiConfig interceptor
  // transparently refreshes the access token and retries the request.
  if (requiresAuth && !authStore.isAuthenticated && !authStore.loading && to.name !== 'login') {
    try {
      await authStore.fetchUserData();
      if (authStore.user) {
        return; // Session restored via refresh token
      }
    } catch {
      // Refresh token expired or invalid — session cannot be restored
    }
    return { name: 'login', query: { redirect: to.fullPath } };
  }

  // Redirect unauthenticated users from protected routes
  if (requiresAuth && !authStore.isAuthenticated) {
    return { name: 'login', query: { redirect: to.fullPath } };
  }

  // Redirect authenticated users away from login/onboarding
  if (authStore.isAuthenticated && authStore.user) {
    if (to.path === '/login' || to.name === 'onboarding') {
      return { name: 'home' };
    }
  }

  // Load feature flags once per session for any authenticated route. Failures
  // are swallowed inside the store; the app falls back to flags-disabled.
  if (authStore.isAuthenticated && authStore.user) {
    const { useFeatureFlagsStore } = await import('@/stores/featureFlags');
    const flagsStore = useFeatureFlagsStore();
    if (!flagsStore.loaded && !flagsStore.loading) {
      void flagsStore.load();
    }

    const { useWorkflowStatesStore } = await import('@/stores/workflowStates');
    const wfStore = useWorkflowStatesStore();
    if (!wfStore.loaded && !wfStore.loading) {
      void wfStore.load();
    }

    // Hydrate the local-first sync runtime. hydrate() is idempotent
    // so re-entry to a protected route after the first call is a
    // no-op. Failure is non-fatal: the runtime degrades to memory-
    // only mode and the views still render — they just don't survive
    // a tab restart and lose live SSE updates.
    if (authStore.user?.uuid) {
      try {
        const [{ hydrate, fetchServerSchemaHash }, { attachSseBridge }] = await Promise.all([
          import('@/sync/lifecycle'),
          import('@/sync/sseBridge'),
        ]);
        const schemaHash = await fetchServerSchemaHash();
        await hydrate(authStore.user.uuid, schemaHash);
        attachSseBridge();
      } catch (e) {
        // eslint-disable-next-line no-console
        console.warn('Failed to hydrate sync runtime', e);
      }
    }
  }
}

/**
 * Check admin access for admin-only routes
 */
async function checkAdminAccess(to: RouteLocationNormalized, _from: RouteLocationNormalized) {
  const requiresAdmin = to.matched.some((record) => record.meta.adminRequired);

  if (requiresAdmin) {
    const { useAuthStore } = await import('@/stores/auth');
    const authStore = useAuthStore();

    if (!authStore.isAdmin) {
      return { name: 'home' };
    }
  }
}

// Register middleware in order of execution
router.beforeEach(checkOnboarding);
router.beforeEach(checkAuthentication);
router.beforeEach(checkAdminAccess);

router.onError((_error) => {
  router.push({
    name: 'error',
    params: {
      code: '500',
      message: 'Something went wrong'
    }
  })
})

export default router