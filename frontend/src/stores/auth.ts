import { defineStore } from 'pinia';
import { logger } from '@/utils/logger';
import { ref, computed } from 'vue';
import axios from 'axios';
import apiClient, { setLoggingOut } from '@/services/apiConfig';
import authService from '@/services/authService';
import router from '@/router';
import type { User, LoginCredentials } from '@nosdesk/core/types';
import { useThemeStore } from './theme';
import { useDateStore } from './dateStore';
import { translate } from '@/i18n';
import { extractErrorMessage } from '@/utils/errors';

// Configure axios to use relative URLs and send cookies
// This will make requests go to the same server that served the frontend
axios.defaults.baseURL = '';
axios.defaults.withCredentials = true; // Enable sending httpOnly cookies with all requests

// UX ONLY: Helper function to check if CSRF token cookie exists
// NOTE: This is for UX optimization only (preventing unnecessary API calls)
// Authentication state is always determined by backend responses
function hasCsrfToken(): boolean {
  return !!document.cookie.match(/csrf_token=([^;]+)/);
}

export const useAuthStore = defineStore('auth', () => {
  const user = ref<User | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const authProvider = ref<string | null>(localStorage.getItem('authProvider'));

  // Track ongoing fetchUserData requests to prevent duplicates
  let fetchUserDataPromise: Promise<User | null> | null = null;

  // Track last fetch attempt to prevent rapid retries on rate limit errors
  let lastFetchAttempt = 0;
  const FETCH_COOLDOWN_MS = 5000; // 5 second cooldown after failed attempts

  // Add MFA state management
  const mfaRequired = ref(false);
  const mfaSetupRequired = ref(false);
  const passkeyMfaRequired = ref(false);
  const mfaUserUuid = ref<string>('');

  // Set auth provider header if available (don't auto-fetch user data)
  // User data will be loaded by router navigation guard when needed
  if (hasCsrfToken() && authProvider.value) {
    axios.defaults.headers.common['X-Auth-Provider'] = authProvider.value;
  }

  // Computed properties
  // UX ONLY: Consider authenticated if CSRF token exists OR user data is loaded
  // The user data check handles the timing gap where cookies are being set but not yet in document.cookie
  // NOTE: Actual authentication is always verified by backend on every request
  const isAuthenticated = computed(() => hasCsrfToken() || !!user.value);
  // Admin tier = platform admin OR workspace owner/admin in the
  // resolved workspace. Technician tier additionally includes
  // workspace agents (can handle tickets). Derived from the W2 role
  // split (platform_role + workspace_role) now that the legacy single
  // `role` field is gone.
  const isAdmin = computed(
    () =>
      user.value?.platform_role === 'platform_admin' ||
      user.value?.workspace_role === 'owner' ||
      user.value?.workspace_role === 'admin'
  );
  const isTechnician = computed(() => isAdmin.value || user.value?.workspace_role === 'agent');
  // Standalone read-only audit role (Item C/D4). Distinct from admin:
  // an audit reviewer can reach only the audit surface, not the rest
  // of the admin panel.
  const isAuditReviewer = computed(() => user.value?.platform_role === 'audit_reviewer');
  // Platform operator (Nosdesk staff on hosted). Distinct from `isAdmin`,
  // which also includes per-workspace owners/admins. Gates cross-tenant
  // operator-only UI (e.g. the inbound dead-letter view).
  const isPlatformAdmin = computed(() => user.value?.platform_role === 'platform_admin');
  const isMicrosoftAuth = computed(() => authProvider.value === 'microsoft');

  // Fetch current user data from the backend
  // NOTE: No CSRF cookie guard here. When cookies expire (15 min), the API call
  // will get a 401, and the interceptor in apiConfig.ts will automatically attempt
  // a refresh using the 7-day refresh token before failing.
  async function fetchUserData() {
    // Return existing promise if already fetching
    if (fetchUserDataPromise) {
      return fetchUserDataPromise;
    }

    // Check cooldown period to prevent rapid retries after failures
    const now = Date.now();
    if (now - lastFetchAttempt < FETCH_COOLDOWN_MS) {
      logger.debug('Fetch user data on cooldown, skipping request');
      return null;
    }

    lastFetchAttempt = now;

    // Create and cache the promise
    fetchUserDataPromise = (async () => {
      try {
        loading.value = true;
        // Only log in development or when explicitly requested
        if (import.meta.env.DEV) {
          logger.debug('Fetching user data...');
        }

        const userData = await authService.getCurrentUser();
        // A confirmed authenticated session ends any prior teardown window.
        setLoggingOut(false);
        user.value = userData;

        // Load theme from user profile
        const themeStore = useThemeStore();
        themeStore.loadThemeFromUser(userData);

        // Seed locale + timezone from the backend's resolved
        // effective_* fields. The settings picker reads the raw
        // userLocale / userTimezone prefs; everything else reads
        // `locale` / `effectiveTimezone`.
        const dateStore = useDateStore();
        dateStore.loadFromUser(userData);

        // Reset cooldown on success
        lastFetchAttempt = 0;
        return userData;
      } catch (err) {
        logger.error('Error fetching user data:', err);

        // Handle specific error cases
        const axiosError = err as { response?: { status: number } };
        if (axiosError.response) {
          const status = axiosError.response.status;

          if (status === 429) {
            // Rate limit error - don't logout, just show error
            logger.warn('Rate limit exceeded. Please wait before retrying.');
            error.value = translate('auth-login-rate-limited', undefined, 'Too many requests. Please wait a moment.');
            throw err;
          } else if (status === 401 || status === 403) {
            // Unauthorized/Forbidden - logout and clear cookies
            logger.debug('Logging out due to authentication error:', status);
            logout();
          } else {
            // Other server errors - keep user logged in
            error.value = translate(
              'error-store-auth-profile-load',
              undefined,
              'Failed to load profile data. Please try again.',
            );
            throw err;
          }
        } else {
          // Network error - keep user logged in
          error.value = translate('auth-login-network-error', undefined, 'Network error. Please check your connection.');
          throw err;
        }

        return null;
      } finally {
        loading.value = false;
        // Clear the promise cache
        fetchUserDataPromise = null;
      }
    })();

    return fetchUserDataPromise;
  }

  // Simplified login - returns boolean, sets MFA state if needed
  async function login(credentials: LoginCredentials): Promise<boolean> {
    loading.value = true;
    error.value = null;
    mfaRequired.value = false;
    mfaSetupRequired.value = false;
    passkeyMfaRequired.value = false;

    try {
      const response = await apiClient.post('/auth/login', credentials);

      // Handle TOTP MFA required
      if (response.data.mfa_required) {
        mfaRequired.value = true;
        mfaUserUuid.value = response.data.user_uuid || '';
        error.value = response.data.message || 'Multi-factor authentication required';
        return false;
      }

      // Handle passkey MFA required
      if (response.data.passkey_mfa_required) {
        passkeyMfaRequired.value = true;
        mfaUserUuid.value = response.data.user_uuid || '';
        error.value = null;
        return false;
      }

      // Handle MFA setup required
      if (response.data.mfa_setup_required) {
        mfaSetupRequired.value = true;
        mfaUserUuid.value = response.data.user_uuid || '';
        // Don't set this as an error - it's expected behavior
        error.value = null;
        return false;
      }

      // Handle successful login (cookies set by backend, csrf_token in response)
      if (response.data.success && response.data.csrf_token) {
        setAuthData(response.data.user);
        router.push('/');
        return true;
      }

      // Handle other cases
      error.value = response.data.message || 'Login failed. Please try again.';
      return false;

    } catch (err) {
      logger.error('Login error:', err);
      error.value = extractErrorMessage(err, 'Login failed. Please check your credentials.');
      return false;
    } finally {
      loading.value = false;
    }
  }

  // Simplified MFA login
  async function verifyMfaAndLogin(email: string, password: string, mfaToken: string): Promise<boolean> {
    loading.value = true;
    error.value = null;

    try {
      logger.debug('🔐 MFA Login: Submitting MFA token...');
      const response = await apiClient.post('/auth/mfa-login', {
        email,
        password,
        mfa_token: mfaToken.trim()
      });

      logger.debug('🔐 MFA Login: Response received', {
        success: response.data.success,
        hasCsrfToken: !!response.data.csrf_token,
        hasUser: !!response.data.user
      });

      if (response.data.success && response.data.csrf_token) {
        logger.debug('🔐 MFA Login: Setting auth data for user', response.data.user);
        setAuthData(response.data.user);

        // Show backup code warning if needed
        if (response.data.mfa_backup_code_used && response.data.requires_backup_code_regeneration) {
          error.value = translate(
            'auth-login-backup-codes-low',
            undefined,
            'Login successful! Please regenerate your backup codes soon, you have 2 or fewer remaining.',
          );
        }

        mfaRequired.value = false;
        mfaUserUuid.value = '';

        logger.debug('🔐 MFA Login: Auth data set, user state:', {
          hasUser: !!user.value,
          isAuthenticated: isAuthenticated.value,
          userName: user.value?.name
        });

        logger.debug('🔐 MFA Login: Attempting redirect to /');
        await router.push('/');
        logger.debug('🔐 MFA Login: Redirect completed');
        return true;
      }

      logger.warn('🔐 MFA Login: Login not successful', response.data);
      error.value = response.data.message || 'MFA verification failed';
      return false;

    } catch (err) {
      logger.error('🔐 MFA Login error:', err);
      const axiosError = err as {
        response?: {
          status?: number;
          data?: { message?: string; error?: string; code?: string; retry_after?: number };
        };
      };
      const status = axiosError.response?.status;
      const data = axiosError.response?.data;

      // Surface the MFA rate limiter explicitly. The backend returns
      // 429 / code RATE_LIMITED once the attempt cap is hit, after which
      // *every* submission is rejected regardless of whether the code is
      // correct — so the user needs to know to wait, not to re-check
      // their code.
      if (status === 429 || data?.code === 'RATE_LIMITED') {
        const seconds = data?.retry_after;
        error.value = seconds
          ? translate(
              'auth-mfa-rate-limited-retry',
              { seconds },
              `Too many attempts. Please try again in ${seconds} seconds.`,
            )
          : translate(
              'auth-mfa-rate-limited',
              undefined,
              'Too many MFA attempts. Please try again later.',
            );
      } else {
        error.value =
          data?.message ||
          data?.error ||
          translate('auth-mfa-failed', undefined, 'MFA verification failed. Please try again.');
      }
      return false;
    } finally {
      loading.value = false;
    }
  }

    // Helper function to set authentication data (tokens are in httpOnly cookies)
    function setAuthData(userData: User) {
      // A fresh authenticated session ends any prior sign-out teardown
      // window, so 401-suppression no longer applies.
      setLoggingOut(false);
      user.value = userData;
      authProvider.value = 'local';
      localStorage.setItem('authProvider', 'local');
      axios.defaults.headers.common['X-Auth-Provider'] = 'local';

      // Load theme + locale/timezone from user profile.
      const themeStore = useThemeStore();
      themeStore.loadThemeFromUser(userData);
      const dateStore = useDateStore();
      dateStore.loadFromUser(userData);
    }

    // MFA Setup for Login - Start setup process for users who need MFA
    async function startMfaSetupLogin(email: string, password: string): Promise<{ secret: string; qr_code: string; backup_codes: string[] } | null> {
      loading.value = true;
      error.value = null;

      try {
        return await authService.setupMFAForLogin({ email, password });
      } catch (err) {
        logger.error('MFA setup error:', err);
        error.value = extractErrorMessage(err, translate('error-store-auth-mfa-setup-start', undefined, 'Failed to start MFA setup. Please try again.'));
        return null;
      } finally {
        loading.value = false;
      }
    }

    // MFA Enable for Login - Complete setup and login. The TOTP secret
    // is stashed server-side at setup time (mfa::stash_setup_secret), so
    // the enable request carries only the verifying token + backup codes.
    async function completeMfaSetupAndLogin(email: string, password: string, token: string, backupCodes: string[]): Promise<boolean> {
      loading.value = true;
      error.value = null;

      try {
        const response = await authService.enableMFAForLogin({
          email,
          password,
          token: token.trim(),
          backup_codes: backupCodes
        });

        if (response.user) {
          setAuthData(response.user);
          mfaSetupRequired.value = false;
          mfaUserUuid.value = '';
          router.push('/');
          return true;
        }
        
        error.value = translate('auth-mfa-setup-failed-retry', undefined, 'MFA setup failed. Please try again.');
        return false;
        
      } catch (err) {
        logger.error('MFA enable login error:', err);
        error.value = extractErrorMessage(err, translate('error-store-auth-mfa-setup-complete', undefined, 'Failed to complete MFA setup. Please try again.'));
        return false;
      } finally {
        loading.value = false;
      }
    }

    // Clear MFA state
    function clearMfaState() {
      mfaRequired.value = false;
      mfaSetupRequired.value = false;
      passkeyMfaRequired.value = false;
      mfaUserUuid.value = '';
    }

  // Handle external auth (Microsoft, etc.) - tokens now in httpOnly cookies
  async function setExternalAuth(tokenStr: string, userData: User | null, provider: string = 'microsoft') {
    user.value = userData;
    authProvider.value = provider;

    localStorage.setItem('authProvider', provider);
    axios.defaults.headers.common['X-Auth-Provider'] = provider;

    // If no user data was provided, fetch it from the backend
    if (!userData) {
      try {
        await fetchUserData();
      } catch (err) {
        logger.error('Failed to fetch user data after external auth:', err);
        // Don't throw error here - authentication was successful
      }
    }

    return true;
  }

  async function logout() {
    // Mark the session as intentionally tearing down so the API layer
    // treats the 401s from any in-flight / settling requests as expected
    // teardown noise rather than failures to log + refresh. Cleared on the
    // next successful sign-in (setAuthData / fetchUserData).
    setLoggingOut(true);

    // Check if user logged in via OIDC provider
    const currentProvider = authProvider.value;
    const isOidcUser = currentProvider === 'oidc';

    try {
      // Call backend logout endpoint to clear cookies
      await authService.logout();
    } catch (err) {
      logger.error('Logout request failed:', err);
      // Continue with frontend logout even if backend call fails
    }

    // Manually clear the csrf_token cookie (not httpOnly, so deletion is possible)
    // This ensures isAuthenticated becomes false immediately
    document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
    document.cookie = 'access_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
    document.cookie = 'refresh_token=; path=/api/auth/refresh; expires=Thu, 01 Jan 1970 00:00:00 GMT';

    // Clear user data
    user.value = null;
    authProvider.value = null;

    // Tear down everything workspace-scoped (config stores, query cache, sync
    // pool, SSE bridge, collab caches) so a different user signing in afterwards
    // doesn't briefly see the previous session's data. The same routine backs
    // the in-app workspace switch. myWorkspaces needs no explicit reset: its
    // query key is account-scoped, so clearing `user` switches it to `anon`.
    try {
      const { resetWorkspaceScopedState } = await import('@/stores/workspaceReset');
      await resetWorkspaceScopedState();
    } catch (e) {
      logger.error('Failed to reset workspace-scoped state on logout', e);
    }

    // Account/session teardown that does NOT belong to the workspace-scoped
    // routine: reset appearance to the application default so the login page
    // shows the brand theme rather than the signed-out user's personal
    // theme/accent. Device-level settings (device-local theme pin, colour-blind
    // mode) are deliberately kept.
    try {
      const { useThemeStore } = await import('@/stores/theme');
      useThemeStore().resetToDefault();
    } catch (e) {
      logger.error('Failed to reset theme on logout', e);
    }

    // Remove from localStorage
    localStorage.removeItem('authProvider');

    // Remove auth provider header
    delete axios.defaults.headers.common['X-Auth-Provider'];

    // For OIDC users, also logout from the identity provider (e.g., Keycloak)
    if (isOidcUser) {
      try {
        const response = await apiClient.post('/auth/oauth/logout', {
          provider_type: 'oidc',
          redirect_uri: window.location.origin + '/login'
        });

        if (response.data.logout_url) {
          // Redirect to OIDC provider's logout endpoint
          window.location.href = response.data.logout_url;
          return; // Full redirect in progress, skip router navigation
        }
      } catch (err) {
        logger.error('OIDC logout request failed:', err);
        // Continue with normal redirect if OIDC logout fails
      }
    }

    // Redirect to login page (for non-OIDC users or if OIDC logout fails)
    router.push('/login');
  }

  // Helper method to set auth provider consistently
  function setAuthProvider(provider: 'local' | 'microsoft' | 'oidc') {
    authProvider.value = provider;
    localStorage.setItem('authProvider', provider);
    axios.defaults.headers.common['X-Auth-Provider'] = provider;
  }

  return {
    user,
    loading,
    error,
    authProvider,
    mfaRequired,
    mfaSetupRequired,
    passkeyMfaRequired,
    mfaUserUuid,
    isAuthenticated,
    isAdmin,
    isTechnician,
    isAuditReviewer,
    isPlatformAdmin,
    isMicrosoftAuth,
    login,
    verifyMfaAndLogin,
    startMfaSetupLogin,
    completeMfaSetupAndLogin,
    clearMfaState,
    logout,
    fetchUserData,
    setExternalAuth,
    setAuthProvider
  };
}); 