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
import { getCollabWsUrl } from '@/utils/collabWsUrl'
import { refreshAccessToken } from './authRefresh'

const cookieAuthStrategy: AuthStrategy = {
  authHeaders(): Record<string, string> {
    const csrf = getCsrfToken()
    return csrf ? { 'X-CSRF-Token': csrf } : {}
  },
  useCredentials: true,
  refresh: refreshAccessToken,
  hasSession() {
    return getCsrfToken() !== null
  },
  // The auth cookies are httpOnly, so JS can't clear them; the server's
  // /auth/logout does. A no-op here keeps the long-standing web behaviour.
  onSessionLost() {},
}

configureTransport({
  baseUrl: import.meta.env.VITE_API_URL || '/api',
  collabWsBaseUrl: getCollabWsUrl(),
  auth: cookieAuthStrategy,
})
