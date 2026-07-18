/**
 * Transport seam for @nosdesk/core.
 *
 * The web app and the (future) Tauri mobile app speak to the same backend but
 * differ in two ways only: the REST/WS base URL, and how a request is
 * authenticated (web: same-origin httpOnly cookies + a CSRF header; mobile:
 * a bearer token from the keychain, no cookies). Everything else, the 50-odd
 * `*Service.ts` callers, the sync runtime, SSE and collab URL building, is
 * identical.
 *
 * This module is that single seam. The host injects an `AuthStrategy` and the
 * base URLs once at bootstrap via `configureTransport`; core code then calls
 * `apiBaseUrl()` / `sseStreamUrl()` / `collabWsBaseUrl()` and
 * `transport().auth.*` without knowing which surface it runs on.
 *
 * Headless invariant: core can't read `window.location` or `import.meta.env`,
 * so all platform-derived values (the relative-vs-absolute base, the ws/wss
 * protocol swap, any env override) are resolved by the host and passed in.
 * The concrete strategies live in their hosts for the same reason: a cookie
 * strategy reads `document.cookie`, a bearer strategy reads native storage,
 * both DOM/platform concerns. Core owns only the contract below.
 */

/**
 * How one surface authenticates a request and recovers a lost session.
 * Implemented per host (web: cookie + CSRF; mobile: bearer token).
 */
export interface AuthStrategy {
  /** Extra headers to attach to every request (e.g. CSRF or Authorization). */
  authHeaders(): Record<string, string>
  /** Send ambient credentials (cookies)? Web: true. Mobile: false. */
  readonly useCredentials: boolean
  /** Rotate the session. Resolves true on success. Hosts dedup concurrent calls. */
  refresh(): Promise<boolean>
  /** Is a session plausibly present? Web: CSRF cookie set. Mobile: token held. */
  hasSession(): boolean
  /** Tear down local session state after an unrecoverable 401. Web: no-op. */
  onSessionLost(): void
  /**
   * Intentional sign-out teardown of the client-held session, distinct from
   * the involuntary `onSessionLost`. Web: clear the JS-accessible auth cookies.
   * Mobile: drop the bearer + keychain refresh token and unregister push. The
   * server-side session revocation happens separately (POST /auth/logout).
   */
  endSession(): Promise<void>
}

export interface TransportConfig {
  /** REST base, no trailing slash. Web: `/api`. Mobile: `https://host/api`. */
  baseUrl: string
  /**
   * Collab y-websocket base including the `/collaboration/ws` suffix; the
   * provider appends `/${docId}`. Host-derived because the ws/wss protocol
   * and origin can only be resolved where `window.location` exists.
   */
  collabWsBaseUrl: string
  auth: AuthStrategy
}

let config: TransportConfig | null = null

// Host-supplied per-request headers beyond auth: the Model-C workspace-selection
// header (`X-Nosdesk-Workspace`) and request-tracing/diagnostics headers
// (correlation id, trace id, SSE client id, auth provider). Each host concern
// registers its own provider and they compose, so both the web `apiConfig`
// interceptor and the mobile interceptor (whose bootstrap clears apiConfig) can
// apply the same union. Empty until a host registers, so a bare config sends none.
const requestHeaderProviders: Array<() => Record<string, string>> = []

/** Wire the active transport. Called once at host bootstrap, before any request. */
export function configureTransport(c: TransportConfig): void {
  config = c
}

/**
 * Register a provider of extra per-request headers (workspace selection,
 * diagnostics, …). Providers compose in registration order; read per-request via
 * `requestHeaders()`. Each owner registers near its source so the timing matches
 * when its value becomes available.
 */
export function addRequestHeaderProvider(provider: () => Record<string, string>): void {
  requestHeaderProviders.push(provider)
}

/** The union of every registered host per-request header, for one request. */
export function requestHeaders(): Record<string, string> {
  const out: Record<string, string> = {}
  for (const provider of requestHeaderProviders) Object.assign(out, provider())
  return out
}

function active(): TransportConfig {
  if (!config) {
    throw new Error(
      '@nosdesk/core transport is not configured: call configureTransport() at bootstrap.',
    )
  }
  return config
}

/** The active transport config (use for `transport().auth`). */
export function transport(): TransportConfig {
  return active()
}

/** REST base URL, e.g. `/api`. */
export function apiBaseUrl(): string {
  return active().baseUrl
}

/** Full SSE stream URL for a pre-built query string (no leading `?`). */
export function sseStreamUrl(queryString: string): string {
  return `${active().baseUrl}/events/stream?${queryString}`
}

/** Collab y-websocket base (provider appends `/${docId}`). */
export function collabWsBaseUrl(): string {
  return active().collabWsBaseUrl
}

/**
 * Resolve a file path for use as an `<img>` / `<audio>` / `<video>` src.
 *
 * Defaults to identity: on web a relative `/api/files/...` resolves against the
 * app origin and the cookie authenticates the direct load. The mobile app
 * overrides this (`configureAssetUrl`) to rewrite the path to the `nosdesk-asset`
 * scheme, which the Tauri Rust handler proxies to the API with the bearer, the
 * webview can't carry auth on a direct resource load and a relative URL would
 * resolve against the wrong (`tauri://localhost`) origin. See
 * mobile/src-tauri/src/asset_proxy.rs.
 */
let assetUrlResolver: (path: string) => string = (path) => path

export function configureAssetUrl(resolver: (path: string) => string): void {
  assetUrlResolver = resolver
}

export function assetUrl(path: string): string {
  return assetUrlResolver(path)
}
