/**
 * Web transport wiring for @nosdesk/core's transport seam.
 *
 * The browser surface authenticates with same-origin httpOnly cookies plus a
 * double-submit CSRF header, and talks to a same-origin (or env-overridden)
 * base URL. This module packages that as the seam's `AuthStrategy` + base URLs
 * and registers them once. It is imported first in `main.ts` so the seam is
 * live before any request fires; the Tauri app will register its own
 * bearer-token equivalent instead.
 */
import { configureTransport, type AuthStrategy } from '@nosdesk/core/transport'
import { getCsrfToken } from '@/utils/csrf'
import { refreshAccessToken } from '@nosdesk/core/services/authRefresh'

// Same-origin by default; an explicit absolute base overrides it.
const baseUrl = import.meta.env.VITE_API_URL || '/api'

/**
 * Base URL for the y-websocket collaboration server (the provider appends
 * `/${docId}`). Resolution order: explicit `VITE_WS_SERVER_URL`, else derive
 * from the REST base (relative → swap to ws/wss against the current origin;
 * absolute → swap the http(s) scheme). Platform-specific (reads
 * `window.location`), so it lives in the host, not core.
 */
function deriveCollabWsBaseUrl(): string {
  const explicit = import.meta.env.VITE_WS_SERVER_URL
  if (explicit) return explicit

  if (baseUrl.startsWith('/')) {
    const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    return `${wsProtocol}//${window.location.host}${baseUrl}/collaboration/ws`
  }
  return baseUrl.replace(/^http/, 'ws') + '/collaboration/ws'
}

/**
 * Clear the JS-readable CSRF cookie, so `hasSession()` (and the store's
 * isAuthenticated) flips false at once. The httpOnly access/refresh cookies
 * are the server's to clear. The name is `__Host-csrf_token` on https (which
 * needs `secure` in the clearing string) and `csrf_token` on plain-http dev.
 */
function clearCsrfCookie(): void {
  const expired = 'expires=Thu, 01 Jan 1970 00:00:00 GMT'
  document.cookie = `__Host-csrf_token=; path=/; secure; ${expired}`
  document.cookie = `csrf_token=; path=/; ${expired}`
}

// The CSRF cookie the last refresh went out with. Cookies are shared by every
// tab, so by the time a refusal lands another tab may have signed in and set
// a new one; only the session that was refused is dropped.
let refreshSentWith: string | null = null

const cookieAuthStrategy: AuthStrategy = {
  authHeaders(): Record<string, string> {
    const csrf = getCsrfToken()
    return csrf ? { 'X-CSRF-Token': csrf } : {}
  },
  useCredentials: true,
  refresh() {
    refreshSentWith = getCsrfToken()
    return refreshAccessToken()
  },
  hasSession() {
    return getCsrfToken() !== null
  },
  // The refresh was rejected: the session is over, so this browser no longer
  // holds one, unless a newer sign-in replaced its CSRF cookie meanwhile. The
  // server sets no cookies on a refusal, so this is the only clearing.
  onSessionLost() {
    if (getCsrfToken() === refreshSentWith) clearCsrfCookie()
  },
  // Intentional sign-out. The server's /auth/logout clears the httpOnly cookies.
  async endSession() {
    clearCsrfCookie()
  },
}

configureTransport({
  baseUrl,
  collabWsBaseUrl: deriveCollabWsBaseUrl(),
  auth: cookieAuthStrategy,
})
