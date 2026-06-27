/**
 * Bearer-token transport wiring for the Tauri app, the mobile twin of the web's
 * `frontend/src/services/transport.ts`.
 *
 * The app authenticates with a session JWT in `Authorization: Bearer` plus
 * `X-Auth-Mode: bearer` (so the backend returns tokens in the body, not
 * cookies). The short-lived access token lives in memory so `authHeaders()` can
 * read it synchronously per request; the long-lived refresh token is mirrored
 * in memory for `hasSession()` and persisted to the keychain (SecureStore) for
 * cold starts. The base URL comes from the selected server (see serverConfig).
 */
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import { apiBaseUrl, configureTransport, type AuthStrategy } from '@nosdesk/core/transport'
import { apiBaseUrlFor, collabWsBaseUrlFor, storeServer } from './serverConfig'
import type { SecureStore } from './secureStore'

// In-memory session state (see module doc for why access stays in memory).
let accessToken: string | null = null
let refreshToken: string | null = null
let store: SecureStore | null = null

interface BearerTokens {
  access_token?: string
  refresh_token?: string
}

/** Register the keychain store. Called once at bootstrap, before configureServer. */
export function setSecureStore(secureStore: SecureStore): void {
  store = secureStore
}

/**
 * Seed the session after a successful login / MFA / passkey finish. The login
 * response (bearer mode) carries `access_token` + `refresh_token` in its body.
 */
export async function setSession(access: string, refresh: string): Promise<void> {
  accessToken = access
  refreshToken = refresh
  await store?.save(refresh)
}

/** Clear the session locally (sign-out, or before switching servers). */
export async function clearSession(): Promise<void> {
  accessToken = null
  refreshToken = null
  await store?.clear()
}

const bearerAuthStrategy: AuthStrategy = {
  authHeaders() {
    const headers: Record<string, string> = { 'X-Auth-Mode': 'bearer' }
    if (accessToken) headers.Authorization = `Bearer ${accessToken}`
    return headers
  },
  // No cookie jar on a tauri:// origin.
  useCredentials: false,
  async refresh() {
    if (!refreshToken) return false
    try {
      // Native fetch (off the webview / off the axios interceptor stack).
      const res = await tauriFetch(`${apiBaseUrl()}/auth/refresh`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-Auth-Mode': 'bearer' },
        body: JSON.stringify({ refresh_token: refreshToken }),
      })
      if (!res.ok) return false
      const data = (await res.json()) as BearerTokens
      if (!data.access_token || !data.refresh_token) return false
      accessToken = data.access_token
      refreshToken = data.refresh_token
      await store?.save(data.refresh_token)
      return true
    } catch {
      return false
    }
  },
  hasSession() {
    return accessToken !== null || refreshToken !== null
  },
  onSessionLost() {
    accessToken = null
    refreshToken = null
    // Fire-and-forget: the interface is synchronous, keychain clear is async.
    void store?.clear()
  },
}

/**
 * Point the transport at a server origin. Loads that server's persisted refresh
 * token so a returning user can transparently refresh into an access token.
 * Does NOT persist the choice (bootstrap uses the stored/default server;
 * `setServer` is the explicit, persisted user choice).
 */
export async function configureServer(origin: string): Promise<void> {
  accessToken = null
  refreshToken = (await store?.load()) ?? null
  configureTransport({
    baseUrl: apiBaseUrlFor(origin),
    collabWsBaseUrl: collabWsBaseUrlFor(origin),
    auth: bearerAuthStrategy,
  })
}

/**
 * The explicit "connect to this server" action for the connect / settings
 * screen: drop any existing session (a different server needs a fresh login),
 * persist the choice, and point the transport at it. Validate the origin with
 * `validateServer` first.
 */
export async function setServer(origin: string): Promise<void> {
  await clearSession()
  storeServer(origin)
  await configureServer(origin)
}
