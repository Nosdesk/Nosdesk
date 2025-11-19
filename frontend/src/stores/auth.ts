import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import axios from 'axios';
import apiClient from '@/services/apiConfig';
import router from '@/router';
import type { User, LoginCredentials } from '@/types';
import { useThemeStore } from './theme';

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
  let fetchUserDataPromise: Promise<any> | null = null;

  // Track last fetch attempt to prevent rapid retries on rate limit errors
  let lastFetchAttempt = 0;
  const FETCH_COOLDOWN_MS = 5000; // 5 second cooldown after failed attempts

  // Add MFA state management
  const mfaRequired = ref(false);
  const mfaSetupRequired = ref(false);
  const mfaUserUuid = ref<string>('');

  // Set auth provider header if available (don't auto-fetch user data)
  // User data will be loaded by router navigation guard when needed
  if (hasCsrfToken() && authProvider.value) {
    axios.defaults.headers.common['X-Auth-Provider'] = authProvider.value;
  }

  // Computed properties
  // UX ONLY: Consider authenticated if we have a CSRF token OR if we have user data loaded
  // The user data check handles the timing gap where cookies are being set but not yet in document.cookie
  // NOTE: Actual authentication is always verified by backend on every request
  const isAuthenticated = computed(() => hasCsrfToken() || !!user.value);
  const isAdmin = computed(() => user.value?.role === 'admin');
  const isTechnician = computed(() => user.value?.role === 'technician' || user.value?.role === 'admin');
  const isMicrosoftAuth = computed(() => authProvider.value === 'microsoft');

  // Fetch current user data from the backend
  async function fetchUserData() {
    if (!hasCsrfToken()) return null;

    // Return existing promise if already fetching
    if (fetchUserDataPromise) {
      return fetchUserDataPromise;
    }

    // Check cooldown period to prevent rapid retries after failures
    const now = Date.now();
    if (now - lastFetchAttempt < FETCH_COOLDOWN_MS) {
      console.log('Fetch user data on cooldown, skipping request');
      return null;
    }

    lastFetchAttempt = now;

    // Create and cache the promise
    fetchUserDataPromise = (async () => {
      try {
        loading.value = true;
        // Only log in development or when explicitly requested
        if (import.meta.env.DEV) {
          console.log('Fetching user data...');
        }

        const response = await apiClient.get('/auth/me');
        user.value = response.data;

        // Load theme from user profile
        const themeStore = useThemeStore();
        themeStore.loadThemeFromUser(response.data);

        // Reset cooldown on success
        lastFetchAttempt = 0;
        return response.data;
      } catch (err: any) {
        console.error('Error fetching user data:', err);

        // Handle specific error cases
        if (err.response) {
          const status = err.response.status;

          if (status === 429) {
            // Rate limit error - don't logout, just show error
            console.warn('Rate limit exceeded. Please wait before retrying.');
            error.value = 'Too many requests. Please wait a moment.';
            throw err;
          } else if (status === 401 || status === 403) {
            // Unauthorized/Forbidden - logout and clear cookies
            console.log('Logging out due to authentication error:', status);
            logout();
          } else {
            // Other server errors - keep user logged in
            error.value = 'Failed to load profile data. Please try again.';
            throw err;
          }
        } else {
          // Network error - keep user logged in
          error.value = 'Network error. Please check your connection.';
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

    try {
      const response = await apiClient.post('/auth/login', credentials);

      // Handle MFA required
      if (response.data.mfa_required) {
        mfaRequired.value = true;
        mfaUserUuid.value = response.data.user_uuid || '';
        error.value = response.data.message || 'Multi-factor authentication required';
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

    } catch (err: any) {
      console.error('Login error:', err);
      error.value = err.response?.data?.message || 'Login failed. Please check your credentials.';
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
      console.log('🔐 MFA Login: Submitting MFA token...');
      const response = await apiClient.post('/auth/mfa-login', {
        email,
        password,
        mfa_token: mfaToken.trim()
      });

      console.log('🔐 MFA Login: Response received', {
        success: response.data.success,
        hasCsrfToken: !!response.data.csrf_token,
        hasUser: !!response.data.user
      });

      if (response.data.success && response.data.csrf_token) {
        console.log('🔐 MFA Login: Setting auth data for user', response.data.user);
        setAuthData(response.data.user);

        // Show backup code warning if needed
        if (response.data.mfa_backup_code_used && response.data.requires_backup_code_regeneration) {
          error.value = 'Login successful! Please regenerate your backup codes soon - you have 2 or fewer remaining.';
        }

        mfaRequired.value = false;
        mfaUserUuid.value = '';

        console.log('🔐 MFA Login: Auth data set, user state:', {
          hasUser: !!user.value,
          isAuthenticated: isAuthenticated.value,
          userName: user.value?.name
        });

        console.log('🔐 MFA Login: Attempting redirect to /');
        await router.push('/');
        console.log('🔐 MFA Login: Redirect completed');
        return true;
      }

      console.warn('🔐 MFA Login: Login not successful', response.data);
      error.value = response.data.message || 'MFA verification failed';
      return false;

    } catch (err: any) {
      console.error('🔐 MFA Login error:', err);
      error.value = err.response?.data?.message || 'MFA verification failed. Please try again.';
      return false;
    } finally {
      loading.value = false;
    }
  }

    // Helper function to set authentication data (tokens are in httpOnly cookies)
    function setAuthData(userData: User) {
      user.value = userData;
      authProvider.value = 'local';
      localStorage.setItem('authProvider', 'local');
      axios.defaults.headers.common['X-Auth-Provider'] = 'local';

      // Load theme from user profile
      const themeStore = useThemeStore();
      themeStore.loadThemeFromUser(userData);
    }

    // MFA Setup for Login - Start setup process for users who need MFA
    async function startMfaSetupLogin(email: string, password: string): Promise<{ secret: string; qr_code: string; backup_codes: string[] } | null> {
      loading.value = true;
      error.value = null;

      try {
        const response = await apiClient.post('/auth/mfa-setup-login', {
          email,
          password
        });

        return response.data;
      } catch (err: any) {
        console.error('MFA setup error:', err);
        error.value = err.response?.data?.message || 'Failed to start MFA setup. Please try again.';
        return null;
      } finally {
        loading.value = false;
      }
    }

    // MFA Enable for Login - Complete setup and login
    async function completeMfaSetupAndLogin(email: string, password: string, token: string, secret: string, backupCodes: string[]): Promise<boolean> {
      loading.value = true;
      error.value = null;

      try {
        const response = await apiClient.post('/auth/mfa-enable-login', {
          email,
          password,
          token: token.trim(),
          secret,
          backup_codes: backupCodes
        });

        if (response.data.success && response.data.csrf_token) {
          setAuthData(response.data.user);
          mfaSetupRequired.value = false;
          mfaUserUuid.value = '';
          router.push('/');
          return true;
        }
        
        error.value = response.data.message || 'MFA setup failed. Please try again.';
        return false;
        
      } catch (err: any) {
        console.error('MFA enable login error:', err);
        error.value = err.response?.data?.message || 'Failed to complete MFA setup. Please try again.';
        return false;
      } finally {
        loading.value = false;
      }
    }

    // Clear MFA state
    function clearMfaState() {
      mfaRequired.value = false;
      mfaSetupRequired.value = false;
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
        console.error('Failed to fetch user data after external auth:', err);
        // Don't throw error here - authentication was successful
      }
    }

    return true;
  }

  async function logout() {
    try {
      // Call backend logout endpoint to clear cookies
      await apiClient.post('/auth/logout');
    } catch (err) {
      console.error('Logout request failed:', err);
      // Continue with frontend logout even if backend call fails
    }

    // Manually clear the csrf_token cookie (it's not httpOnly so we can delete it)
    // This ensures isAuthenticated becomes false immediately
    document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
    document.cookie = 'access_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
    document.cookie = 'refresh_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';

    // Clear user data
    user.value = null;
    authProvider.value = null;

    // Remove from localStorage
    localStorage.removeItem('authProvider');

    // Remove auth provider header
    delete axios.defaults.headers.common['X-Auth-Provider'];

    // Redirect to login page
    router.push('/login');
  }

  // Helper method to set auth provider consistently
  function setAuthProvider(provider: 'local' | 'microsoft') {
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
    mfaUserUuid,
    isAuthenticated,
    isAdmin,
    isTechnician,
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