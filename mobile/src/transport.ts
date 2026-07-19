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
import { invoke } from '@tauri-apps/api/core'
import {
  apiBaseUrl,
  configureAssetUrl,
  configureTransport,
  type AuthStrategy,
} from '@nosdesk/core/transport'
import { resetInstanceConfig } from '@nosdesk/core/services/instanceConfig'
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

// Asset proxy: a webview <img>/<audio>/<video> load of a workspace-scoped file
// can't carry the bearer, and a relative URL resolves against tauri://localhost.
// So file paths are rewritten to a custom scheme that the Rust handler proxies
// to the API with auth (see src-tauri/src/asset_proxy.rs). iOS sees the scheme
// directly; Android rewrites it to an http://<scheme>.localhost origin.
const ASSET_SCHEME_PREFIX = /android/i.test(navigator.userAgent)
  ? 'http://nosdesk-asset.localhost'
  : 'nosdesk-asset://localhost'

configureAssetUrl((path) => (path.startsWith('/') ? `${ASSET_SCHEME_PREFIX}${path}` : path))

/**
 * Push the current bearer + API origin to the Rust asset proxy so it can
 * authenticate proxied file fetches. Called on every session change.
 */
function syncAssetProxy(): void {
  let baseUrl: string | null = null
  try {
    baseUrl = new URL(apiBaseUrl()).origin
  } catch {
    // Transport not configured yet; the proxy keeps its last base.
  }
  void invoke('set_asset_proxy_session', { token: accessToken, baseUrl }).catch(() => {})
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
  syncAssetProxy()
}

/** Clear the session locally (sign-out, or before switching servers). */
export async function clearSession(): Promise<void> {
  // Best-effort: revoke this device's push token while the bearer is still
  // valid, before dropping it. Dynamic import keeps the transport bootstrap
  // decoupled from the push module; a no-op if nothing was registered.
  try {
    const { unregisterForPush } = await import('./push')
    await unregisterForPush()
  } catch {
    // Sign-out must never block on push cleanup.
  }
  accessToken = null
  refreshToken = null
  await store?.clear()
  syncAssetProxy()
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
      syncAssetProxy()
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
    syncAssetProxy()
  },
  // Intentional sign-out: drop the bearer + keychain refresh token and
  // unregister this device for push. `clearSession` already does exactly this
  // teardown, so the seam just delegates to it.
  endSession() {
    return clearSession()
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
  syncAssetProxy()
  // The instance config (routing topology) is server-specific. Forget any value
  // resolved against the previous/default server so the next fetch resolves it
  // against THIS one, else a stale/failed 'host' result strands path-mode
  // servers with no workspace slug (every workspace request → NoWorkspaceSelected).
  resetInstanceConfig()
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
