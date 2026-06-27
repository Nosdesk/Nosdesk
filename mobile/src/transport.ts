/**
 * Bearer-token transport wiring for the Tauri app, the mobile twin of the
 * web's `frontend/src/services/transport.ts`.
 *
 * The app authenticates with a session JWT in `Authorization: Bearer` plus
 * `X-Auth-Mode: bearer` (so the backend returns tokens in the body instead of
 * setting cookies). The short-lived access token lives in memory so
 * `authHeaders()` can read it synchronously per request; the long-lived refresh
 * token is mirrored in memory for `hasSession()` and persisted to the keychain
 * (see SecureStore) so a cold start can re-establish the session.
 */
import axios from 'axios'
import { configureTransport, type AuthStrategy } from '@nosdesk/core/transport'
import type { SecureStore } from './secureStore'

// In-memory session state (see module doc for why access stays in memory).
let accessToken: string | null = null
let refreshToken: string | null = null

let baseUrl = ''
let store: SecureStore | null = null

interface BearerTokens {
  access_token?: string
  refresh_token?: string
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

/** Clear the session locally (sign-out). */
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
      const res = await axios.post<BearerTokens>(
        `${baseUrl}/auth/refresh`,
        { refresh_token: refreshToken },
        { headers: { 'X-Auth-Mode': 'bearer' }, withCredentials: false },
      )
      const access = res.data.access_token
      const refresh = res.data.refresh_token
      if (!access || !refresh) return false
      accessToken = access
      refreshToken = refresh
      await store?.save(refresh)
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

export interface MobileTransportOptions {
  /** Absolute REST base, e.g. `https://app.nosdesk.com/api`. */
  apiBaseUrl: string
  /** Absolute collab y-websocket base, including the `/collaboration/ws` suffix. */
  collabWsBaseUrl: string
  /** Keychain-backed store for the refresh token. */
  secureStore: SecureStore
}

/**
 * Register the bearer transport with `@nosdesk/core`. Loads any persisted
 * refresh token from the keychain first, so a returning user's first
 * authenticated request can transparently refresh into an access token.
 */
export async function setupTransport(opts: MobileTransportOptions): Promise<void> {
  baseUrl = opts.apiBaseUrl
  store = opts.secureStore
  refreshToken = await store.load()
  configureTransport({
    baseUrl: opts.apiBaseUrl,
    collabWsBaseUrl: opts.collabWsBaseUrl,
    auth: bearerAuthStrategy,
  })
}
