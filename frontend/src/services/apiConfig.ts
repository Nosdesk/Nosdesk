import axios from 'axios';

// API Configuration with Conditional Logging
// 
// Logging behavior:
// - Minimal logging by default (errors only)
// - Verbose request/response logging enabled when localStorage['api-verbose-logging'] = 'true'
// - To enable verbose logging: localStorage.setItem('api-verbose-logging', 'true')
// - To disable: localStorage.removeItem('api-verbose-logging')

// Set API URL based on environment
export const API_URL = import.meta.env.VITE_API_URL || '/api';

// Create axios instance with default config
const apiClient = axios.create({
  baseURL: API_URL,
  withCredentials: true, // Enable sending cookies with requests
  headers: {
    'Content-Type': 'application/json',
  },
});

// Helper function to get CSRF token from cookies
function getCsrfToken(): string | null {
  const match = document.cookie.match(/csrf_token=([^;]+)/);
  return match ? match[1] : null;
}

// Add request interceptor for CSRF token
apiClient.interceptors.request.use(
  (config) => {
    // Add CSRF token to header for state-changing requests
    const csrfToken = getCsrfToken();

    if (csrfToken) {
      config.headers['X-CSRF-Token'] = csrfToken;
    }

    // Auth provider header (if available in localStorage)
    const authProvider = localStorage.getItem('authProvider');
    if (authProvider) {
      config.headers['X-Auth-Provider'] = authProvider;
    }

    // TEMPORARY: ALWAYS log CSRF token status for debugging (not just DEV mode)
    console.log(`🔒 API Request to ${config.url}:`, {
      method: config.method,
      hasCsrfToken: !!csrfToken,
      csrfToken: csrfToken ? csrfToken.substring(0, 10) + '...' : 'none',
      headerValue: config.headers['X-CSRF-Token'] ? config.headers['X-CSRF-Token'].substring(0, 10) + '...' : 'missing',
      cookieCount: document.cookie.split(';').filter(c => c.trim()).length,
      withCredentials: config.withCredentials
    });

    // Only log requests in development mode and when verbose logging is enabled
    if (import.meta.env.DEV && window.localStorage.getItem('api-verbose-logging') === 'true') {
      console.log(`API Request to ${config.url}:`, {
        method: config.method,
        headers: config.headers,
        data: config.data,
      });
    }

    return config;
  },
  (error) => {
    return Promise.reject(error);
  }
);

// Add response interceptor for error handling
apiClient.interceptors.response.use(
  (response) => {
    // Only log responses in development mode and when verbose logging is enabled
    if (import.meta.env.DEV && window.localStorage.getItem('api-verbose-logging') === 'true') {
      // Get the call stack to identify which component made the request
      const stack = new Error().stack;
      const caller = stack?.split('\n')[4]?.trim() || 'Unknown caller';
      
      console.log(`API Response from ${response.config.url}:`, { 
        status: response.status,
        data: response.data,
        caller: caller
      });
    }
    
    return response;
  },
  (error) => {
    // Handle authentication errors
    if (error.response && error.response.status === 401) {
      // If we're already on the login page, don't redirect
      if (!window.location.pathname.includes('/login')) {
        // Clear auth provider and redirect to login (cookies cleared by backend)
        localStorage.removeItem('authProvider');
        window.location.href = '/login';
      }
    }
    
    // Log error details in development
    if (import.meta.env.DEV) {
      console.error('API Error:', {
        message: error.message,
        config: error.config,
        response: error.response ? {
          status: error.response.status,
          data: error.response.data
        } : 'No response'
      });
    }
    
    return Promise.reject(error);
  }
);

export default apiClient; 