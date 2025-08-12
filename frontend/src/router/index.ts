import { createRouter, createWebHistory } from 'vue-router'
import DashboardView from '../views/DashboardView.vue'
import TicketView from '../views/TicketView.vue'
import LoginView from '../views/LoginView.vue'
import OnboardingView from '../views/OnboardingView.vue'
import ErrorView from '../views/ErrorView.vue'
import TicketsListView from '../views/TicketsListView.vue'
import ProjectsView from '../views/ProjectsView.vue'
import ProjectDetailView from '../views/ProjectDetailView.vue'
import UserProfileView from '../views/UserProfileView.vue'
import DocumentationPageView from '@/views/DocumentationPageView.vue'
import ProfileSettingsView from '@/views/ProfileSettingsView.vue'
import PDFViewerView from '@/views/PDFViewerView.vue'
import MicrosoftConfigView from '@/views/MicrosoftConfigView.vue'
import authService from '@/services/authService'

declare module 'vue-router' {
  interface RouteMeta {
    requiresAuth: boolean;
    title?: string;
    layout?: string;
    adminRequired?: boolean;
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
      path: '/',
      name: 'home',
      component: DashboardView,
      meta: {
        requiresAuth: true,
        title: 'Dashboard'
      }
    },
    {
      path: '/tickets',
      name: 'tickets',
      component: TicketsListView,
      meta: {
        requiresAuth: true,
        title: 'Tickets'
      }
    },
    {
      path: '/tickets/:id',
      name: 'ticket-view',
      component: TicketView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'View Ticket'
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
        title: 'Projects'
      }
    },
    {
      path: '/projects/:id',
      name: 'project-detail',
      component: ProjectDetailView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Project Details'
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
        title: 'Users'
      }
    },
    {
      path: '/devices',
      name: 'devices',
      component: () => import('../views/DevicesListView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Devices'
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
      },
      beforeEnter: (to) => {
        to.meta.title = `Device #${to.params.id}`
      }
    },
    {
      path: '/documentation',
      name: 'documentation',
      component: DocumentationPageView,
      meta: {
        requiresAuth: true,
        title: 'Documentation'
      }
    },
    {
      path: '/documentation/:path',
      name: 'documentation-page',
      component: DocumentationPageView,
      meta: {
        requiresAuth: true,
        title: 'Documentation'
      },
      props: true,
      beforeEnter: async (to) => {
        // Set a generic title initially
        to.meta.title = 'Documentation';
        
        // Handle ticket notes
        if (to.query.ticketId) {
          to.meta.title = `Ticket #${to.query.ticketId} Notes`;
          return;
        }

        // Set title based on the path
        if (to.params.path) {
          const path = to.params.path.toString();
          
          // If it's a category, format the title
          if (path.startsWith('category-')) {
            const categoryName = path.replace('category-', '').replace(/\d+$/, '');
            if (categoryName) {
              to.meta.title = categoryName
                .split('-')
                .map(word => word.charAt(0).toUpperCase() + word.slice(1))
                .join(' ');
            }
          } else {
            // For regular pages, format the slug as a title
            // Check if it's a numeric ID (legacy support) or a slug
            if (!isNaN(Number(path))) {
              // It's a numeric ID, we'll let the component handle the title
              to.meta.title = 'Documentation';
            } else {
              // It's a slug, format it as a title
              to.meta.title = path
                .split('-')
                .map(word => word.charAt(0).toUpperCase() + word.slice(1))
                .join(' ');
            }
          }
        }
      }
    },
    {
      path: '/profile',
      name: 'profile-redirect',
      component: () => null, // Empty component since we're redirecting
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
      path: '/admin/settings',
      name: 'admin-settings',
      component: () => import('../views/AdminSettingsView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Administration',
        adminRequired: true
      }
    },
    {
      path: '/admin/auth-providers',
      name: 'admin-auth-providers',
      component: () => import('../views/AuthProvidersView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Authentication Providers',
        adminRequired: true
      }
    },
    {
      path: '/admin/data-import',
      name: 'admin-data-import',
      component: () => import('../views/DataImportView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Data Import',
        adminRequired: true
      }
    },
    {
      path: '/admin/data-import/microsoft-graph',
      name: 'admin-microsoft-graph',
      component: () => import('../views/MicrosoftGraphView.vue'),
      meta: {
        requiresAuth: true,
        title: 'Microsoft Graph Connection',
        adminRequired: true
      }
    },
    {
      path: '/admin/data-import/csv',
      name: 'admin-csv-import',
      component: () => import('../views/CsvImportView.vue'),
      meta: {
        requiresAuth: true,
        title: 'CSV Import',
        adminRequired: true
      }
    },
    {
      path: '/auth/microsoft/callback',
      name: 'microsoft-callback',
      component: () => import('../views/auth/MicrosoftCallbackView.vue'),
      meta: {
        layout: 'blank',
        requiresAuth: false,
        title: 'Authenticating...'
      }
    },
    {
      path: '/pdf-viewer',
      name: 'pdf-viewer',
      component: PDFViewerView,
      meta: {
        requiresAuth: true,
        title: 'PDF Viewer'
      }
    },
    {
      path: '/admin/microsoft-config/:providerId?',
      name: 'admin-microsoft-config',
      component: MicrosoftConfigView,
      props: true,
      meta: {
        requiresAuth: true,
        title: 'Microsoft Entra Configuration',
        adminRequired: true
      }
    },
    {
      path: '/admin/system-settings',
      name: 'admin-system-settings',
      component: () => import('../views/SystemSettingsView.vue'),
      meta: {
        requiresAuth: true,
        title: 'System Settings',
        adminRequired: true
      }
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/error/404'
    }
  ],
})

// Update document title on navigation
router.beforeResolve((to, from, next) => {
  let title = 'Nosdesk';
  
  if (to.meta?.title) {
    title = to.meta.title as string;
    // For ticket view, append the ticket ID if available
    if (to.name === 'ticket-view' && to.params.id) {
      title = `Ticket #${to.params.id}`;
    }
  } else if (to.name) {
    title = to.name.toString()
      .split('-')
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  }
  
  document.title = `${title} | Nosdesk`;
  if (to.meta) {
    to.meta.title = title;
  }

  next();
});

// Authentication guard
router.beforeEach(async (to, from, next) => {
  // Import auth store inside the guard to avoid circular dependencies
  const { useAuthStore } = await import('@/stores/auth');
  const authStore = useAuthStore();

  // Security: Check for onboarding requirements FIRST (before any other checks)
  // This ensures new users are always directed to onboarding regardless of auth state
  if (to.name !== 'onboarding' && to.name !== 'error' && to.name !== 'login') {
    try {
      // Security: Prevent excessive setup checks during navigation
      const setupStatus = await authService.checkSetupStatus();
      if (setupStatus.requires_setup) {
        // Security: Check if we're already trying to redirect to onboarding
        if (from.name === 'onboarding') {
          console.warn('⚠️  Router: Already on onboarding, preventing redirect loop');
          next(false);
          return;
        }
        
        // System requires setup, redirect to onboarding
        console.log('🔄 Router: System requires setup, redirecting to onboarding');
        next({ name: 'onboarding' });
        return;
      }
    } catch (error) {
      console.error('Failed to check setup status during navigation:', error);
      // Security: If we can't check setup status, continue normally
      // The error will be handled by the onboarding component if needed
    }
  }

  // Check for unsaved changes
  // @ts-ignore
  if (window.hasUnsavedChanges && !window.confirm('You have unsaved changes. Are you sure you want to leave?')) {
    next(false);
    return;
  }

  // Check if the route requires authentication
  const requiresAuth = to.matched.some(record => record.meta.requiresAuth);
  const requiresAdmin = to.matched.some(record => record.meta.adminRequired);
  
  // Use auth store to check authentication and admin status
  const isAuthenticated = authStore.isAuthenticated;
  
  // Fetch user data if authenticated but no user data loaded yet
  if (isAuthenticated && !authStore.user && !authStore.loading) {
    try {
      await authStore.fetchUserData();
    } catch (error) {
      console.error('Failed to fetch user data during navigation:', error);
      // If we can't fetch user data, log out and redirect to login
      authStore.logout();
      next({ name: 'login', query: { redirect: to.fullPath } });
      return;
    }
  }
  
  // Use auth store to check admin status
  const isAdmin = authStore.isAdmin;
  
  if (requiresAuth && !isAuthenticated) {
    // Redirect to login page if not authenticated
    next({ name: 'login', query: { redirect: to.fullPath } });
  } else if (requiresAdmin && !isAdmin) {
    // Redirect to home if not an admin
    next({ name: 'home' });
  } else if (to.path === '/login' && isAuthenticated) {
    // Redirect to home if already authenticated and trying to access login page
    next({ name: 'home' });
  } else if (to.name === 'onboarding' && isAuthenticated) {
    // If user is authenticated and trying to access onboarding, redirect to home
    next({ name: 'home' });
  } else {
    // Continue to the route
    next();
  }
});

router.onError((error) => {
  router.push({
    name: 'error',
    params: {
      code: '500',
      message: 'Something went wrong'
    }
  })
})

export default router