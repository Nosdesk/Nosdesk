import axios from 'axios';
import { logger } from '@/utils/logger';
import { createErrorFromResponse } from '@/utils/errors';
import { ErrorTracker } from '@/utils/errorTracking';
import { getSSEClientId } from '@/services/sseService';
import { activeWorkspaceSlug as getActiveWorkspaceSlug } from '@/services/activeWorkspace';
import { getSessionId as getDiagnosticsSessionId } from '@/services/diagnostics/session';
import { pushApi as pushApiBreadcrumb } from '@/services/diagnostics/breadcrumbs';
import { getCsrfToken } from '@/utils/csrf';
// Shared, in-flight-deduplicated access-token refresh, used by both this
// axios client and the raw-fetch sync runtime so the two never fire two
// concurrent (token-rotating) refreshes against each other.
import { refreshAccessToken } from './authRefresh';

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

// Set while an intentional sign-out is in progress, and kept set until the
// next successful sign-in. During this window the session is gone on
// purpose, so any 401s from requests still settling (or from an
// unauthenticated page) are expected teardown noise: the interceptor skips
// the token-refresh dance and the error-level logging for them. The auth
// store flips this via `setLoggingOut`.
let loggingOut = false;
export function setLoggingOut(value: boolean): void {
  loggingOut = value;
}
export function isLoggingOut(): boolean {
  return loggingOut;
}

/** True on the unauthenticated shell pages, where a 401 is expected. */
function onPublicAuthPage(): boolean {
  return (
    window.location.pathname.includes('/login') ||
    window.location.pathname.includes('/onboarding')
  );
}

function subscribeTokenRefresh(callback: (success: boolean) => void) {
  refreshSubscribers.push(callback);
}

function onRefreshComplete(success: boolean) {
  refreshSubscribers.forEach(callback => callback(success));
  refreshSubscribers = [];
}


// Redirect to login using Vue Router to preserve SPA history stack.
//
// Note: we do NOT clear cookies here. The auth cookies (access_token,
// refresh_token) are httpOnly, so JavaScript can't delete them anyway —
// the old `document.cookie = ...` lines were silent no-ops that gave a
// false impression of "logging out". This path only runs on genuine
// session expiry (the refresh endpoint rejected us / refresh failed),
// where those cookies are already invalid server-side, and a successful
// login re-issues fresh ones. Server-initiated logout (/api/auth/logout)
// is what actually clears the httpOnly cookies.
function redirectToLogin() {
  sessionStorage.setItem('redirecting-to-login', 'true');
  localStorage.removeItem('authProvider');

  setTimeout(async () => {
    sessionStorage.removeItem('redirecting-to-login');
    const { default: router } = await import('@/router');
    router.push('/login');
  }, 100);
}

// Add request interceptor for CSRF token and correlation ID
apiClient.interceptors.request.use(
  (config) => {
    // Generate correlation ID for request tracing
    if (!currentCorrelationId) {
      currentCorrelationId = generateCorrelationId();
    }
    config.headers['X-Correlation-ID'] = currentCorrelationId;

    // Per-tab diagnostics session id. Backend tracing spans pick it
    // up so `grep <session_id>` correlates a bug report's session to
    // every backend request from the same tab.
    config.headers['X-Nosdesk-Trace-Id'] = getDiagnosticsSessionId();

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

    // SSE client ID for echo suppression (Pusher-style pattern)
    const sseClientId = getSSEClientId();
    if (sseClientId) {
      config.headers['X-SSE-Client-Id'] = sseClientId;
    }

    // Selected workspace (Model C single-origin / path mode). The router keeps
    // this in sync with the URL slug; it's null in host mode, where the backend
    // resolves the workspace from the Host instead and ignores this header.
    const workspaceSlug = getActiveWorkspaceSlug();
    if (workspaceSlug) {
      config.headers['X-Nosdesk-Workspace'] = workspaceSlug;
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

    // Diagnostic breadcrumb. The pushApi helper filters /bug-reports
    // and /auth/refresh internally so a report submission doesn't
    // record itself, and the refresh ping doesn't drown the trail.
    pushApiBreadcrumb(
      response.config.method?.toUpperCase() ?? 'GET',
      response.config.url ?? '',
      response.status,
    );

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
    // Cancellations short-circuit. RequestManager aborts in-flight
    // requests when a new one shares the same key (the routine
    // cancel-prior-and-replace pattern), and Vue components abort
    // their fetches on unmount. Both surface here as axios cancels.
    // They aren't errors — logging them as such fills the console
    // with red text that masks real failures, and the auth /
    // refresh path below should also skip cancellations.
    if (axios.isCancel(error)) {
      return Promise.reject(error);
    }

    // Expected auth teardown. An intentional sign-out (or any request that
    // 401s while we're on a public auth page) must not trigger the
    // token-refresh dance or log error-level noise: the session is gone on
    // purpose. Treat the 401 as expected and reject quietly so a deliberate
    // logout doesn't spam the console.
    if (error.response?.status === 401 && (loggingOut || onPublicAuthPage())) {
      currentCorrelationId = null;
      // First-run setup 401s carry a machine-readable `code` in the body
      // (BOOTSTRAP_TOKEN_*) that OnboardingView localises. Reject the raw
      // error so `response.data` survives — the typed AppError from
      // createErrorFromResponse drops both the body and the code.
      if (error.config?.url?.includes('/auth/setup/')) {
        return Promise.reject(error);
      }
      return Promise.reject(createErrorFromResponse(error));
    }

    const correlationId = error.response?.headers['x-correlation-id'] || currentCorrelationId;

    // Diagnostic breadcrumb on failure. Same allowlist filter as the
    // success branch; status is the HTTP status if the response made
    // it back, undefined for network/abort.
    pushApiBreadcrumb(
      error.config?.method?.toUpperCase() ?? 'GET',
      error.config?.url ?? '',
      error.response?.status,
    );

    // Create typed error
    const appError = createErrorFromResponse(error);

    // Log error with appropriate level. Skip the log-forwarding endpoint
    // itself: logging its failure would be re-captured by the remote
    // logger's console interceptor and re-queued, creating a ~1Hz POST loop
    // that exhausts the shared rate limit and can 429 auth / MFA setup.
    if (!error.config?.url?.includes('/debug/frontend-logs')) {
      logger.error(`API Error: ${appError.message}`, {
        correlationId,
        endpoint: error.config?.url,
        method: error.config?.method,
        status: error.response?.status,
        data: error.response?.data
      });
    }

    // Handle authentication errors (401)
    if (error.response?.status === 401) {
      const originalRequest = error.config;

      // First-run setup returns 401 when the bootstrap token is
      // missing, expired, or wrong — not because the session died.
      // Skip refresh/redirect so OnboardingView can read the
      // machine-readable `code` and show a localised message.
      if (originalRequest.url?.includes('/auth/setup/')) {
        return Promise.reject(error);
      }

      // A 401 from the refresh endpoint itself means the session
      // genuinely can't be renewed -> send the user to login.
      if (originalRequest.url?.includes('/auth/refresh')) {
        const onPublicAuthPage =
          window.location.pathname.includes('/login') ||
          window.location.pathname.includes('/onboarding');
        if (!onPublicAuthPage && !sessionStorage.getItem('redirecting-to-login')) {
          logger.warn('Session expired (refresh rejected) - redirecting to login', { correlationId });
          redirectToLogin();
        }
        return Promise.reject(appError);
      }

      // Already refreshed once and retried, yet the endpoint still 401s.
      // That's an endpoint-specific authorization problem, NOT an expired
      // session, so surface it to the caller instead of logging the user
      // out. A single misbehaving endpoint must not nuke the whole
      // session (this is what turned the collaboration-scope 401 into a
      // full-page bounce to the dashboard).
      if (originalRequest._retry) {
        logger.warn('Endpoint 401 after a successful token refresh; not treating as session expiry', {
          correlationId,
          endpoint: originalRequest.url,
        });
        return Promise.reject(appError);
      }

      // First 401 on a normal request: try to refresh the token once.
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

          // Redirect to login (but not from first-run setup)
          const onPublicAuthPage =
            window.location.pathname.includes('/login') ||
            window.location.pathname.includes('/onboarding');
          if (!onPublicAuthPage && !sessionStorage.getItem('redirecting-to-login')) {
            redirectToLogin();
          }
        }
      } catch {
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