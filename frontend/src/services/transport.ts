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
import { refreshAccessToken } from './authRefresh'

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
  baseUrl,
  collabWsBaseUrl: deriveCollabWsBaseUrl(),
  auth: cookieAuthStrategy,
})
