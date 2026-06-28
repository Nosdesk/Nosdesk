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

// Host-supplied per-request headers beyond auth, currently the Model-C
// workspace-selection header (`X-Nosdesk-Workspace`). The web `apiConfig`
// interceptor reads the same source directly; the mobile interceptor reads it
// through this seam because its bootstrap clears the web interceptor. Defaults
// to none, so a host that never registers a provider is unaffected.
let selectionHeadersProvider: () => Record<string, string> = () => ({})

/** Wire the active transport. Called once at host bootstrap, before any request. */
export function configureTransport(c: TransportConfig): void {
  config = c
}

/**
 * Register the host's selection-header provider (which workspace this client is
 * acting in). Called once at bootstrap; read per-request via `selectionHeaders()`.
 */
export function setSelectionHeaders(provider: () => Record<string, string>): void {
  selectionHeadersProvider = provider
}

/** The host's current selection headers (workspace-selection, etc.) for a request. */
export function selectionHeaders(): Record<string, string> {
  return selectionHeadersProvider()
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
