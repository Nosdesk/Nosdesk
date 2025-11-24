import axios from 'axios';
import { logger } from '@/utils/logger';
import { createErrorFromResponse } from '@/utils/errors';
import { ErrorTracker } from '@/utils/errorTracking';

// API Configuration with Structured Logging and Error Handling
//
// Logging behavior:
// - Production: ERROR level only, structured logs sent to backend
// - Development: DEBUG level, verbose logging when localStorage['api-verbose-logging'] = 'true'
// - To enable verbose logging: localStorage.setItem('api-verbose-logging', 'true')

// Set API URL based on environment
export const API_URL = import.meta.env.VITE_API_URL || '/api';

// Correlation ID management for request tracing
let currentCorrelationId: string | null = null;

export function generateCorrelationId(): string {
  return `req-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

export function setCorrelationId(id: string) {
  currentCorrelationId = id;
  logger.setCorrelationId(id);
}

// Create axios instance with default config
const apiClient = axios.create({
  baseURL: API_URL,
  withCredentials: true, // Enable sending cookies with requests
  headers: {
    'Content-Type': 'application/json',
  },
});

// Token refresh state to prevent multiple simultaneous refresh attempts
let isRefreshing = false;
let refreshSubscribers: ((success: boolean) => void)[] = [];

function subscribeTokenRefresh(callback: (success: boolean) => void) {
  refreshSubscribers.push(callback);
}

function onRefreshComplete(success: boolean) {
  refreshSubscribers.forEach(callback => callback(success));
  refreshSubscribers = [];
}

async function refreshAccessToken(): Promise<boolean> {
  try {
    const response = await axios.post(`${API_URL}/auth/refresh`, {}, {
      withCredentials: true,
    });
    return response.status === 200;
  } catch (error) {
    return false;
  }
}

// Helper function to get CSRF token from cookies
function getCsrfToken(): string | null {
  const match = document.cookie.match(/csrf_token=([^;]+)/);
  return match ? match[1] : null;
}

// Add request interceptor for CSRF token and correlation ID
apiClient.interceptors.request.use(
  (config) => {
    // Generate correlation ID for request tracing
    if (!currentCorrelationId) {
      currentCorrelationId = generateCorrelationId();
    }
    config.headers['X-Correlation-ID'] = currentCorrelationId;

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

    // Verbose logging (development only)
    if (import.meta.env.DEV && localStorage.getItem('api-verbose-logging') === 'true') {
      logger.debug('API Request', {
        method: config.method,
        url: config.url,
        correlationId: currentCorrelationId,
        headers: config.headers,
        data: config.data
      });
    } else {
      // Minimal production logging
      logger.debug(`${config.method?.toUpperCase()} ${config.url}`, {
        correlationId: currentCorrelationId
      });
    }

    return config;
  },
  (error) => {
    logger.error('Request interceptor error', { error });
    return Promise.reject(error);
  }
);

// Add response interceptor for error handling
apiClient.interceptors.response.use(
  (response) => {
    // Extract correlation ID from response
    const correlationId = response.headers['x-correlation-id'];
    if (correlationId) {
      setCorrelationId(correlationId);
    }

    // Verbose logging (development only)
    if (import.meta.env.DEV && localStorage.getItem('api-verbose-logging') === 'true') {
      logger.debug('API Response', {
        status: response.status,
        url: response.config.url,
        correlationId,
        data: response.data
      });
    }

    // Reset correlation ID after successful request
    currentCorrelationId = null;

    return response;
  },
  async (error) => {
    const correlationId = error.response?.headers['x-correlation-id'] || currentCorrelationId;

    // Create typed error
    const appError = createErrorFromResponse(error);

    // Log error with appropriate level
    logger.error(`API Error: ${appError.message}`, {
      correlationId,
      endpoint: error.config?.url,
      method: error.config?.method,
      status: error.response?.status,
      data: error.response?.data
    });

    // Handle authentication errors (401)
    if (error.response?.status === 401) {
      const originalRequest = error.config;

      // Don't retry refresh requests or already retried requests
      if (originalRequest.url?.includes('/auth/refresh') || originalRequest._retry) {
        // Refresh failed or already retried - redirect to login
        if (!window.location.pathname.includes('/login') && !sessionStorage.getItem('redirecting-to-login')) {
          logger.warn('Session expired - redirecting to login', {
            correlationId
          });

          sessionStorage.setItem('redirecting-to-login', 'true');

          // Clear auth cookies
          document.cookie = 'access_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
          document.cookie = 'refresh_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
          document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';

          localStorage.removeItem('authProvider');

          setTimeout(() => {
            sessionStorage.removeItem('redirecting-to-login');
            window.location.href = '/login';
          }, 100);
        }
        return Promise.reject(appError);
      }

      // Mark request as retried
      originalRequest._retry = true;

      // If already refreshing, queue this request
      if (isRefreshing) {
        return new Promise((resolve, reject) => {
          subscribeTokenRefresh((success) => {
            if (success) {
              resolve(apiClient(originalRequest));
            } else {
              reject(appError);
            }
          });
        });
      }

      // Attempt to refresh token
      isRefreshing = true;

      try {
        const refreshSuccess = await refreshAccessToken();

        if (refreshSuccess) {
          logger.debug('Token refreshed successfully', { correlationId });
          onRefreshComplete(true);
          isRefreshing = false;

          // Retry original request
          return apiClient(originalRequest);
        } else {
          logger.warn('Token refresh failed', { correlationId });
          onRefreshComplete(false);
          isRefreshing = false;

          // Redirect to login
          if (!window.location.pathname.includes('/login') && !sessionStorage.getItem('redirecting-to-login')) {
            sessionStorage.setItem('redirecting-to-login', 'true');

            document.cookie = 'access_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
            document.cookie = 'refresh_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
            document.cookie = 'csrf_token=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';

            localStorage.removeItem('authProvider');

            setTimeout(() => {
              sessionStorage.removeItem('redirecting-to-login');
              window.location.href = '/login';
            }, 100);
          }
        }
      } catch (refreshError) {
        onRefreshComplete(false);
        isRefreshing = false;
      }
      // Don't send 401 errors to error tracking (expected behavior)
    } else if (error.response?.status === 403) {
      // Permission error
      logger.warn('Permission denied', {
        endpoint: error.config?.url,
        correlationId
      });
    } else if (error.response?.status >= 500) {
      // Server error - track in production
      ErrorTracker.captureException(appError, {
        correlationId,
        endpoint: error.config?.url
      });
    } else if (!error.response) {
      // Network error
      logger.error('Network error', {
        message: error.message,
        correlationId
      });
    }

    // Reset correlation ID
    currentCorrelationId = null;

    return Promise.reject(appError);
  }
);

export default apiClient; 