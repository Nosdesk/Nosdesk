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
import WorkspaceCyclesView from '../views/WorkspaceCyclesView.vue'
import CycleDetailView from '../views/CycleDetailView.vue'
import ProjectGanttView from '../views/ProjectGanttView.vue'
import ProjectCyclesView from '../views/ProjectCyclesView.vue'
import AssetPlannerView from '../views/AssetPlannerView.vue'
import UserProfileView from '../views/UserProfileView.vue'
import DocumentationIndexView from '@/views/DocumentationIndexView.vue'
import DocumentView from '@/views/DocumentView.vue'
import ProfileSettingsView from '@/views/ProfileSettingsView.vue'
import PDFViewerView from '@/views/PDFViewerView.vue'
import authService from '@/services/authService'
import { useInboxLoader } from '@/loaders/inboxLoader'
import { useTicketsListLoader } from '@/loaders/ticketsListLoader'
import { useTicketDetailLoader } from '@/loaders/ticketDetailLoader'
import { translate } from '@/i18n'
import { useBrandingStore } from '@/stores/branding'
import type { Page, Article } from '@/services/documentationService'

declare module 'vue-router' {
  interface RouteMeta {
    /// Optional because children of an authenticated parent inherit
    /// `requiresAuth` via `to.matched.some(...)`. Set explicitly only
    /// to override the inherited value (e.g. a public child of a
    /// protected layout).
    requiresAuth?: boolean;
    title?: string;
    titleKey?: string;
    titleKeyArgs?: Record<string, string | number>;
    layout?: string;
    adminRequired?: boolean;
    /** Within an adminRequired subtree, also allow the standalone
     * audit_reviewer role to reach this route (Item C/D4). */
    auditReviewerAllowed?: boolean;
    createButtonText?: string;
    createButtonTextKey?: string;
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
        titleKey: 'route-title-login'
      }
    },
    {
      path: '/reset-password',
      name: 'reset-password',
      component: PasswordResetView,
      meta: {
        layout: 'blank',
        requiresAuth: false,
        titleKey: 'route-title-reset-password'
      }
    },
    {
      path: '/mfa-setup',
      name: 'mfa-setup',
      component: () => import('@/views/MFASetupView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        titleKey: 'route-title-mfa-setup'
      }
    },
    {
      path: '/accept-invitation',
      name: 'accept-invitation',
      component: () => import('@/views/AcceptInvitationView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        titleKey: 'route-title-accept-invitation'
      }
    },
    {
      path: '/onboarding',
      name: 'onboarding',
      component: OnboardingView,
      meta: {
        layout: 'blank',
        requiresAuth: false,
        titleKey: 'route-title-onboarding'
      }
    },
    {
      path: '/submit-ticket',
      name: 'guest-submit-ticket',
      component: () => import('@/views/public/GuestTicketSubmitView.vue'),
      meta: { layout: 'blank', requiresAuth: false, titleKey: 'route-title-guest-submit-ticket' }
    },
    {
      path: '/ticket-status/:token',
      name: 'guest-ticket-status',
      component: () => import('@/views/public/GuestTicketStatusView.vue'),
      props: true,
      meta: { layout: 'blank', requiresAuth: false, titleKey: 'route-title-guest-ticket-status' }
    },
    {
      path: '/docs',
      name: 'public-docs-list',
      component: () => import('@/views/public/PublicDocsView.vue'),
      meta: { layout: 'blank', requiresAuth: false, titleKey: 'route-title-documentation' }
    },
    {
      path: '/docs/:slug',
      name: 'public-doc',
      component: () => import('@/views/public/PublicDocView.vue'),
      props: true,
      meta: { layout: 'blank', requiresAuth: false, titleKey: 'route-title-documentation' }
    },
    {
      path: '/help',
      name: 'public-help',
      component: () => import('@/views/public/HelpView.vue'),
      meta: { layout: 'blank', requiresAuth: false, titleKey: 'route-title-help' }
    },
    {
      path: '/',
      name: 'home',
      component: DashboardView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-dashboard',
        createButtonTextKey: 'header-create-ticket',
        createButtonIcon: 'ticket',
      }
    },
    {
      path: '/inbox',
      name: 'inbox',
      component: () => import('@/views/NotificationInboxView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-inbox',
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
        titleKey: 'route-title-tickets',
        createButtonTextKey: 'header-create-ticket',
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
        titleKey: 'route-title-ticket-view',
        createButtonTextKey: 'header-create-ticket',
        createButtonIcon: 'ticket',
        // Pre-fetches the ticket payload during navigation so the
        // view mounts with data already in the Pinia Colada cache;
        // useTicketData reads through the same cache key on mount.
        loaders: [useTicketDetailLoader],
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
        titleKey: 'route-title-user-profile'
      },
      beforeEnter: (to) => {
        // Set a generic title initially, the component will update it after fetching the user
        to.meta.titleKey = 'route-title-user-profile'
      }
    },
    {
      path: '/users/:uuid/settings/:section?',
      name: 'user-settings',
      component: ProfileSettingsView,
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-user-settings',
        adminRequired: true
      },
      beforeEnter: (to) => {
        // Update title based on section
        const section = to.params.section as string;
        const sectionTitleKeys: Record<string, string> = {
          profile: 'route-title-user-settings-profile',
          appearance: 'route-title-user-settings-appearance',
          notifications: 'route-title-user-settings-notifications',
          security: 'route-title-user-settings-security'
        };

        if (section && sectionTitleKeys[section]) {
          to.meta.titleKey = sectionTitleKeys[section];
        } else {
          // No section param means base settings URL = profile section
          to.meta.titleKey = 'route-title-user-settings-profile';
        }
      }
    },
    {
      path: '/projects',
      name: 'projects',
      component: ProjectsView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-projects',
        createButtonTextKey: 'header-create-project',
        createButtonIcon: 'project',
      }
    },
    {
      path: '/cycles',
      name: 'workspace-cycles',
      component: WorkspaceCyclesView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-cycles',
      }
    },
    {
      path: '/cycles/:uuid',
      name: 'cycle-detail',
      component: CycleDetailView,
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-cycle-detail',
      }
    },
    {
      path: '/projects/:id/gantt',
      name: 'project-gantt',
      component: ProjectGanttView,
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-project-gantt',
      }
    },
    {
      path: '/projects/:id/cycles',
      name: 'project-cycles',
      component: ProjectCyclesView,
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-cycles',
      }
    },
    {
      // Planner moves to `/assets/planner` so `/assets` (singular)
      // can be the inventory list page. Existing `/assets` deep
      // links now hit a redirect via the route below.
      path: '/assets/planner',
      name: 'asset-planner',
      component: AssetPlannerView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-asset-planner',
      }
    },
    {
      path: '/projects/:id',
      name: 'project-detail',
      component: ProjectDetailView,
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-project-detail',
        createButtonTextKey: 'header-add-ticket',
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
        titleKey: 'route-title-error'
      }
    },
    {
      path: '/users',
      name: 'users',
      component: () => import('../views/UsersListView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-users',
        createButtonTextKey: 'header-create-user',
        createButtonIcon: 'user',
      }
    },
    // Assets (formerly /devices). The list / create / detail
    // views are the same components rebranded; `/devices*` paths
    // resolve to a redirect below so existing deep links and
    // bookmarks keep working.
    {
      path: '/assets',
      name: 'assets',
      component: () => import('../views/AssetsListView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-assets',
        createButtonTextKey: 'header-create-asset',
        createButtonIcon: 'device',
      }
    },
    {
      path: '/assets/new',
      name: 'asset-create',
      component: () => import('../views/AssetView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-asset-create'
      }
    },
    {
      path: '/assets/:id',
      name: 'asset-view',
      component: () => import('../views/AssetView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-asset-view'
      }
    },
    // Legacy redirects. Anyone landing on the old /devices paths
    // (bookmarks, external integrations, in-app links we haven't
    // migrated yet) bounces to the new /assets equivalent so the
    // rename is transparent.
    { path: '/devices', redirect: '/assets' },
    { path: '/devices/new', redirect: '/assets/new' },
    {
      path: '/devices/:id',
      redirect: (to) => ({ path: `/assets/${to.params.id}` }),
    },
    {
      path: '/documentation',
      name: 'documentation',
      component: DocumentationIndexView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-documentation',
        createButtonTextKey: 'header-create-document',
        createButtonIcon: 'document',
      }
    },
    {
      path: '/documentation/drafts',
      name: 'documentation-drafts',
      component: () => import('../views/DocumentationDraftsView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-documentation-drafts'
      }
    },
    {
      path: '/documentation/collections/:slug',
      name: 'collection-view',
      component: () => import('../views/CollectionView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-collection'
      }
    },
    {
      path: '/documentation/archived',
      name: 'documentation-archived',
      component: () => import('../views/DocumentationArchivedView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-documentation-archived'
      }
    },
    {
      path: '/documentation/trash',
      name: 'documentation-trash',
      component: () => import('../views/DocumentationTrashView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-documentation-trash'
      }
    },
    {
      path: '/documentation/gaps',
      name: 'documentation-gaps',
      component: () => import('../views/DocumentationGapsView.vue'),
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-knowledge-gaps'
      }
    },
    {
      path: '/documentation/gaps/:id',
      name: 'documentation-gap-detail',
      component: () => import('../views/DocumentationGapsView.vue'),
      props: true,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-knowledge-gaps'
      }
    },
    {
      path: '/documentation/:path',
      name: 'documentation-page',
      component: DocumentView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-documentation'
      },
      props: true,
      beforeEnter: async (to) => {
        // Set a generic title initially
        to.meta.titleKey = 'route-title-documentation';
        to.meta.title = undefined;
        to.meta.titleKeyArgs = undefined;
        to.meta.preloadedDocument = undefined;

        // Handle ticket notes, preloaded inside component (needs different data shape)
        if (to.query.ticketId) {
          to.meta.titleKey = 'route-title-ticket-notes';
          to.meta.titleKeyArgs = { id: String(to.query.ticketId) };
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
                else return '/documentation'; // not found, redirect
              }
              to.meta.preloadedDocument = doc;
              // Document title is user content, not translatable. Set as
              // literal `title`; consumers prefer titleKey, so clear it.
              if (doc.title) {
                to.meta.title = doc.title;
                to.meta.titleKey = undefined;
              }
            } else {
              return '/documentation'; // not found, redirect
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
        titleKey: 'route-title-profile'
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
        titleKey: 'route-title-profile-settings'
      },
      beforeEnter: (to) => {
        // Update title based on section
        const section = to.params.section as string;
        const sectionTitleKeys: Record<string, string> = {
          profile: 'route-title-profile-settings-profile',
          appearance: 'route-title-profile-settings-appearance',
          notifications: 'route-title-profile-settings-notifications',
          security: 'route-title-profile-settings-security'
        };

        if (section && sectionTitleKeys[section]) {
          to.meta.titleKey = sectionTitleKeys[section];
        } else {
          // No section param means base /profile/settings URL = profile section
          to.meta.titleKey = 'route-title-profile-settings-profile';
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
          meta: { titleKey: 'route-title-administration' }
        },
        { path: 'settings', redirect: '/admin' },
        {
          path: 'groups',
          name: 'admin-groups',
          component: () => import('../views/GroupsManagementView.vue'),
          meta: { titleKey: 'route-title-admin-groups' }
        },
        {
          path: 'groups/:uuid/configure',
          name: 'group-configuration',
          component: () => import('../views/GroupConfigurationView.vue'),
          props: true,
          meta: { titleKey: 'route-title-group-configuration' }
        },
        {
          path: 'categories',
          name: 'admin-categories',
          component: () => import('../views/CategoriesManagementView.vue'),
          meta: { titleKey: 'route-title-admin-categories' }
        },
        {
          path: 'assignment-rules',
          name: 'admin-assignment-rules',
          component: () => import('../views/AssignmentRulesView.vue'),
          meta: { titleKey: 'route-title-admin-assignment-rules' }
        },
        {
          path: 'workflow',
          name: 'admin-workflow',
          component: () => import('../views/admin/WorkflowStatesView.vue'),
          meta: { titleKey: 'route-title-admin-workflow' }
        },
        {
          path: 'asset-kinds',
          name: 'admin-asset-kinds',
          component: () => import('../views/admin/AssetKindsView.vue'),
          meta: { titleKey: 'route-title-admin-asset-kinds' }
        },
        {
          path: 'asset-kinds/new',
          name: 'admin-asset-kinds-new',
          component: () => import('../views/admin/AssetKindEditView.vue'),
          meta: { titleKey: 'route-title-admin-asset-kinds-new' }
        },
        {
          path: 'asset-kinds/:id(\\d+)',
          name: 'admin-asset-kinds-edit',
          component: () => import('../views/admin/AssetKindEditView.vue'),
          meta: { titleKey: 'route-title-admin-asset-kinds-edit' }
        },
        {
          path: 'api-tokens',
          name: 'admin-api-tokens',
          component: () => import('../views/ApiTokensView.vue'),
          meta: { titleKey: 'route-title-admin-api-tokens' }
        },
        // Workspaces lifecycle + member management (Phase 4 W1/W3).
        // Both views are scaffolded stubs — data layer wired, template
        // ready for Cursor to flesh out the UI.
        {
          path: 'workspaces',
          name: 'admin-workspaces',
          component: () => import('../views/admin/AdminWorkspacesView.vue'),
          meta: { titleKey: 'route-title-admin-workspaces' }
        },
        {
          path: 'workspaces/:id(\\d+)/members',
          name: 'admin-workspace-members',
          component: () => import('../views/admin/AdminWorkspaceMembersView.vue'),
          props: true,
          meta: { titleKey: 'route-title-admin-workspace-members' }
        },
        {
          path: 'canned-responses',
          name: 'admin-canned-responses',
          component: () => import('../views/CannedResponsesView.vue'),
          meta: { titleKey: 'route-title-admin-canned-responses' }
        },
        {
          path: 'canned-responses/new',
          name: 'admin-canned-responses-new',
          component: () => import('../views/CannedResponseEditView.vue'),
          meta: { titleKey: 'route-title-admin-canned-responses-new' }
        },
        {
          path: 'canned-responses/:id(\\d+)',
          name: 'admin-canned-responses-edit',
          component: () => import('../views/CannedResponseEditView.vue'),
          meta: { titleKey: 'route-title-admin-canned-responses-edit' }
        },
        {
          path: 'webhooks',
          name: 'admin-webhooks',
          component: () => import('../views/WebhooksView.vue'),
          meta: { titleKey: 'route-title-admin-webhooks' }
        },
        {
          // Was previously a top-level `/admin/sla` route, which
          // unmounted AdminLayout (and its sidebar) on every visit
          // and silently bypassed the admin gate (the standalone
          // route used `requiresAdmin` instead of `adminRequired`,
          // so the nav guard never triggered). Nesting under
          // `/admin` inherits both the layout shell and the
          // parent's `adminRequired: true` meta.
          path: 'sla',
          name: 'admin-sla',
          component: () => import('../views/SlaAdminView.vue'),
          meta: { titleKey: 'route-title-admin-sla' }
        },
        {
          path: 'plugins',
          name: 'admin-plugins',
          component: () => import('../views/admin/plugins/PluginListView.vue'),
          meta: { titleKey: 'route-title-admin-plugins' }
        },
        {
          path: 'plugins/registry',
          name: 'admin-plugin-registry',
          component: () => import('../views/PluginRegistryView.vue'),
          meta: { titleKey: 'route-title-admin-plugin-registry' }
        },
        {
          path: 'plugins/install',
          name: 'admin-plugin-sideload',
          component: () => import('../views/admin/plugins/PluginSideloadView.vue'),
          meta: { titleKey: 'route-title-admin-plugin-sideload' }
        },
        {
          path: 'plugins/:uuid',
          name: 'admin-plugin-detail',
          component: () => import('../views/admin/plugins/PluginDetailView.vue'),
          meta: { titleKey: 'route-title-admin-plugin-detail' }
        },
        {
          path: 'auth-providers',
          name: 'admin-auth-providers',
          component: () => import('../views/AuthProvidersView.vue'),
          meta: { titleKey: 'route-title-admin-auth-providers' }
        },
        {
          path: 'search',
          name: 'admin-search',
          component: () => import('../views/SearchManagementView.vue'),
          meta: { titleKey: 'route-title-admin-search' }
        },
        {
          path: 'system-settings',
          name: 'admin-system-settings',
          component: () => import('../views/SystemSettingsView.vue'),
          meta: { titleKey: 'route-title-admin-system-settings' }
        },
        {
          path: 'settings/branding',
          name: 'admin-branding',
          component: () => import('../views/BrandingSettingsView.vue'),
          meta: { titleKey: 'route-title-admin-branding' }
        },
        {
          // Item C/W5: unified audit feed (all three substrates).
          // Reachable by admins and the standalone audit_reviewer role.
          path: 'audit',
          name: 'admin-audit',
          component: () => import('../views/admin/AdminAuditView.vue'),
          meta: { titleKey: 'route-title-admin-audit-log', auditReviewerAllowed: true }
        },
        {
          // The old tier-3-only view is superseded by the unified feed.
          path: 'audit-log',
          redirect: { name: 'admin-audit' }
        },
        {
          path: 'email-queue',
          name: 'admin-email-queue',
          component: () => import('../views/admin/EmailQueueView.vue'),
          meta: { titleKey: 'route-title-admin-email-queue' }
        },
        {
          path: 'email-suppressions',
          name: 'admin-email-suppressions',
          component: () => import('../views/admin/EmailSuppressionsView.vue'),
          meta: { titleKey: 'route-title-admin-email-suppressions' }
        },
        {
          path: 'guest-access',
          name: 'admin-guest-access',
          component: () => import('../views/GuestAccessSettingsView.vue'),
          meta: { titleKey: 'route-title-admin-guest-access', requiresAuth: true, adminRequired: true }
        },
        {
          path: 'email-settings',
          name: 'admin-email-settings',
          component: () => import('../views/EmailSettingsView.vue'),
          meta: { titleKey: 'route-title-admin-email-settings' }
        },
        {
          path: 'channels/email',
          name: 'admin-channels-email',
          component: () => import('../views/ChannelsEmailSettingsView.vue'),
          meta: { titleKey: 'route-title-admin-channels-email', requiresAuth: true, adminRequired: true }
        },
        {
          path: 'data-import',
          name: 'admin-data-import',
          component: () => import('../views/DataImportView.vue'),
          meta: { titleKey: 'route-title-admin-data-import' }
        },
        {
          path: 'data-import/microsoft-graph',
          name: 'admin-microsoft-graph',
          component: () => import('../views/MicrosoftGraphView.vue'),
          meta: { titleKey: 'route-title-admin-microsoft-graph' }
        },
        {
          path: 'data-import/csv',
          name: 'admin-csv-import',
          component: () => import('../views/CsvImportView.vue'),
          meta: { titleKey: 'route-title-admin-csv-import' }
        },
        {
          path: 'backup-restore',
          name: 'admin-backup-restore',
          component: () => import('../views/BackupRestoreView.vue'),
          meta: { titleKey: 'route-title-admin-backup-restore' }
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
        titleKey: 'route-title-group-detail'
      }
    },
    {
      path: '/auth/microsoft/callback',
      name: 'microsoft-callback',
      component: () => import('../views/auth/OAuthCallbackView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        titleKey: 'route-title-authenticating',
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
        titleKey: 'route-title-authenticating',
        oauthProvider: 'oidc'
      }
    },
    {
      path: '/pdf-viewer',
      name: 'pdf-viewer',
      component: PDFViewerView,
      meta: {
        requiresAuth: true,
        titleKey: 'route-title-pdf-viewer',
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

  // Pull the workspace-configured product name out of the branding
  // store. Pinia stores work outside `setup()` once Pinia is installed,
  // so this is safe inside a router guard. Falls back to "Nosdesk"
  // when the store hasn't initialised (the computed inside the store
  // already handles its own fallback).
  const branding = useBrandingStore();

  let title: string | undefined;

  // Prefer `titleKey` so locale-aware tab titles update on navigation;
  // fall back to `title` for any route that hasn't been migrated. The
  // useTitleManager composable runs alongside this and may override
  // with view-specific titles (e.g. "#123 Fix the bug").
  const titleKey = to.meta?.titleKey as string | undefined;
  const titleKeyArgs = to.meta?.titleKeyArgs as Record<string, string | number> | undefined;
  if (titleKey) {
    title = translate(titleKey, titleKeyArgs);
  } else if (to.meta?.title) {
    title = to.meta.title as string;
  } else if (to.name) {
    title = to.name.toString()
      .split('-')
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  }

  // `getPageTitle(undefined)` returns the bare app name, matching the
  // previous behaviour when no segment was derived; with a segment it
  // appends `| <app name>`. Branding store is the single source of
  // truth for the product label so workspace-renamed installs no
  // longer show "Nosdesk" in the browser tab.
  document.title = branding.getPageTitle(title);
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

    if (authStore.isAdmin) return;

    // The audit_reviewer role may reach only routes explicitly flagged
    // auditReviewerAllowed (the unified audit feed); every other admin
    // route still redirects them home.
    const auditAllowed = to.matched.some((record) => record.meta.auditReviewerAllowed);
    if (auditAllowed && authStore.isAuditReviewer) return;

    return { name: 'home' };
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