/**
 * Short-lived connection token for the collaborative-editing WebSocket.
 *
 * A browser WebSocket can't set an `Authorization` header or send a
 * cross-origin cookie, so the collab socket authenticates with a query-param
 * token (mirroring the SSE token). The token is workspace-bound (Model C), so it
 * is reset on workspace switch / logout via `resetCollabToken()`.
 */
import apiClient from '@nosdesk/core/apiClient';

interface CollabTokenResponse {
  token: string;
  expires_in: number;
  /** Seconds before expiry at which to re-mint. Served by the backend. */
  refresh_buffer?: number;
}

let cached: { token: string; expiresAt: number } | null = null;

// Refetch a little before expiry so a long editing session never connects with
// an about-to-expire token. The backend serves this alongside `expires_in`
// (both derive from one constant there) because the token TTL is deliberately
// short: a buffer hardcoded larger than the TTL would mean the cache never hits
// and every connect re-mints. Falls back to half the TTL if an older backend
// omits the field.
const FALLBACK_BUFFER_RATIO = 0.5;

/**
 * Get a collab connection token, cached until near its expiry. Bound to the
 * request's active workspace, so callers must `resetCollabToken()` on a
 * workspace switch (the WS rejects a token whose workspace doesn't match the doc).
 */
export async function getCollabToken(): Promise<string> {
  const now = Date.now();
  if (cached && now < cached.expiresAt) {
    return cached.token;
  }
  const { data } = await apiClient.post<CollabTokenResponse>('/collaboration/token');
  const bufferSecs = data.refresh_buffer ?? data.expires_in * FALLBACK_BUFFER_RATIO;
  // Store the moment we should stop using it, not the raw expiry, so the
  // buffer is applied once here rather than at every read.
  cached = {
    token: data.token,
    expiresAt: now + Math.max(0, data.expires_in - bufferSecs) * 1000,
  };
  return cached.token;
}

/** Drop the cached token (workspace switch / logout). */
export function resetCollabToken(): void {
  cached = null;
}
