import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import axios from 'axios';
import router from '@/router';

// Configure axios to use relative URLs
// This will make requests go to the same server that served the frontend
axios.defaults.baseURL = '';

interface User {
  id: number;
  uuid: string;
  name: string;
  email: string;
  role: string;
  pronouns?: string | null;
  avatar_url?: string | null;
  banner_url?: string | null;
  avatar_thumb?: string | null;
  created_at: string;
  updated_at: string;
}

interface LoginCredentials {
  email: string;
  password: string;
}

export const useAuthStore = defineStore('auth', () => {
  const token = ref<string | null>(localStorage.getItem('token'));
  const refreshToken = ref<string | null>(localStorage.getItem('refreshToken'));
  const user = ref<User | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const authProvider = ref<string | null>(localStorage.getItem('authProvider'));
  
  // Track ongoing fetchUserData requests to prevent duplicates
  let fetchUserDataPromise: Promise<any> | null = null;

  // Add MFA state management
  const mfaRequired = ref(false);
  const mfaSetupRequired = ref(false);
  const mfaUserUuid = ref<string>('');

  // Set up axios auth header if token exists
  if (token.value) {
    axios.defaults.headers.common['Authorization'] = `Bearer ${token.value}`;
    
    // Set auth provider header if available
    if (authProvider.value) {
      axios.defaults.headers.common['X-Auth-Provider'] = authProvider.value;
    }
    
    // Load user data from backend when store initializes with a token
    // Only if we don't already have user data and not already loading
    if (!user.value && !loading.value) {
      fetchUserData().catch(err => {
        console.error('Failed to load initial user data:', err);
      });
    }
  }

  // Computed properties
  const isAuthenticated = computed(() => !!token.value);
  const isAdmin = computed(() => user.value?.role === 'admin');
  const isTechnician = computed(() => user.value?.role === 'technician' || user.value?.role === 'admin');
  const isMicrosoftAuth = computed(() => authProvider.value === 'microsoft');

  // Helper to detect token type
  function detectTokenType(tokenStr: string): string {
    // Microsoft Entra tokens typically have distinctive characteristics
    if (tokenStr.length > 500 && tokenStr.includes('eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1')) {
      return 'microsoft';
    }
    return 'local';
  }

  // Fetch current user data from the backend
  async function fetchUserData() {
    if (!token.value) return null;

    // Return existing promise if already fetching
    if (fetchUserDataPromise) {
      return fetchUserDataPromise;
    }

    // Create and cache the promise
    fetchUserDataPromise = (async () => {
      try {
        loading.value = true;
        // Only log in development or when explicitly requested
        if (import.meta.env.DEV) {
          console.log('Fetching user data...');
        }
        
        const headers: Record<string, string> = {
          'Authorization': `Bearer ${token.value}`
        };
        
        // Add provider header if we have it or can detect it
        if (authProvider.value) {
          headers['X-Auth-Provider'] = authProvider.value;
        } else if (token.value) {
          // Try to detect token type
          const detectedProvider = detectTokenType(token.value);
          if (detectedProvider === 'microsoft') {
            headers['X-Auth-Provider'] = 'microsoft';
            authProvider.value = 'microsoft';
            localStorage.setItem('authProvider', 'microsoft');
          }
        }
        
        const response = await axios.get('/api/auth/me', { headers });
        user.value = response.data;
        return response.data;
      } catch (err: any) {
        console.error('Error fetching user data:', err);
        
        // Only logout on unauthorized errors, not on network/server errors
        if (err.response && (err.response.status === 401 || err.response.status === 403)) {
          console.log('Logging out due to authentication error:', err.response.status);
          // Token is definitely invalid or expired
          logout();
        } else {
          // For other errors (network, server, etc.), just throw the error
          // but keep the user logged in
          error.value = 'Failed to load profile data. Please try again.';
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
      const response = await axios.post('/api/auth/login', credentials);
      
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
      
      // Handle successful login
      if (response.data.success && response.data.token) {
        setAuthData(response.data.token, response.data.user, response.data.refresh_token);
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
      const response = await axios.post('/api/auth/mfa-login', {
        email,
        password,
        mfa_token: mfaToken.trim()
      });
      
              if (response.data.success && response.data.token) {
          setAuthData(response.data.token, response.data.user, response.data.refresh_token);
          
          // Show backup code warning if needed
          if (response.data.mfa_backup_code_used && response.data.requires_backup_code_regeneration) {
            error.value = 'Login successful! Please regenerate your backup codes soon - you have 2 or fewer remaining.';
          }
          
          mfaRequired.value = false;
          mfaUserUuid.value = '';
          router.push('/');
          return true;
        }
        
        error.value = response.data.message || 'MFA verification failed';
        return false;
        
      } catch (err: any) {
        console.error('MFA login error:', err);
        error.value = err.response?.data?.message || 'MFA verification failed. Please try again.';
        return false;
      } finally {
        loading.value = false;
      }
    }

    // Helper function to set authentication data
    function setAuthData(tokenValue: string, userData: User, refreshTokenValue?: string) {
      token.value = tokenValue;
      user.value = userData;

      localStorage.setItem('token', tokenValue);

      // Store refresh token if provided
      if (refreshTokenValue) {
        refreshToken.value = refreshTokenValue;
        localStorage.setItem('refreshToken', refreshTokenValue);
      }

      authProvider.value = 'local';
      localStorage.setItem('authProvider', 'local');

      axios.defaults.headers.common['Authorization'] = `Bearer ${tokenValue}`;
      axios.defaults.headers.common['X-Auth-Provider'] = 'local';
    }

    // MFA Setup for Login - Start setup process for users who need MFA
    async function startMfaSetupLogin(email: string, password: string): Promise<{ secret: string; qr_code: string; backup_codes: string[] } | null> {
      loading.value = true;
      error.value = null;
      
      try {
        const response = await axios.post('/api/auth/mfa-setup-login', {
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
        const response = await axios.post('/api/auth/mfa-enable-login', {
          email,
          password,
          token: token.trim(),
          secret,
          backup_codes: backupCodes
        });
        
        if (response.data.success && response.data.token) {
          setAuthData(response.data.token, response.data.user, response.data.refresh_token);
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

  // Handle external auth (Microsoft, etc.)
  async function setExternalAuth(tokenStr: string, userData: User | null, provider: string = 'microsoft') {
    token.value = tokenStr;
    user.value = userData;
    authProvider.value = provider;
    
    if (token.value) {
      localStorage.setItem('token', token.value);
      localStorage.setItem('authProvider', provider);
      axios.defaults.headers.common['Authorization'] = `Bearer ${token.value}`;
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
    }
    
    return true;
  }

  function logout() {
    // Clear token and user
    token.value = null;
    refreshToken.value = null;
    user.value = null;
    authProvider.value = null;

    // Remove from localStorage
    localStorage.removeItem('token');
    localStorage.removeItem('refreshToken');
    localStorage.removeItem('authProvider');

    // Remove Authorization header
    delete axios.defaults.headers.common['Authorization'];
    delete axios.defaults.headers.common['X-Auth-Provider'];

    // Redirect to login page
    router.push('/login');
  }

  return {
    token,
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
    setExternalAuth
  };
}); 