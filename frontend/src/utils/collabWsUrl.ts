/**
 * Build the base URL for the y-websocket collaboration server.
 *
 * Centralised so the CollaborativeEditor and any prefetch
 * call sites (RouterLink hover handlers) agree on the URL,
 * and so the URL-derivation rules live in one place.
 *
 * Resolution order:
 *   1. `VITE_WS_SERVER_URL` env var (explicit override).
 *   2. `VITE_API_URL` env var (defaults to `/api`):
 *      - relative path → derive from current origin with the
 *        appropriate ws/wss protocol.
 *      - absolute URL → swap http(s) → ws(s).
 *
 * y-websocket appends `/${docId}` to the URL we return, so this
 * is the *base*, not the per-doc endpoint.
 */
export function getCollabWsUrl(): string {
  const explicit = import.meta.env.VITE_WS_SERVER_URL
  if (explicit) return explicit

  const apiUrl = import.meta.env.VITE_API_URL || '/api'
  if (apiUrl.startsWith('/')) {
    const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    return `${wsProtocol}//${window.location.host}${apiUrl}/collaboration/ws`
  }
  return apiUrl.replace(/^http/, 'ws') + '/collaboration/ws'
}
