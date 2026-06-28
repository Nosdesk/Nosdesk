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
}

let cached: { token: string; expiresAt: number } | null = null;

// Refetch a little before expiry so a long editing session never connects with
// an about-to-expire token.
const REFRESH_BUFFER_MS = 5 * 60 * 1000;

/**
 * Get a collab connection token, cached until near its expiry. Bound to the
 * request's active workspace, so callers must `resetCollabToken()` on a
 * workspace switch (the WS rejects a token whose workspace doesn't match the doc).
 */
export async function getCollabToken(): Promise<string> {
  const now = Date.now();
  if (cached && now < cached.expiresAt - REFRESH_BUFFER_MS) {
    return cached.token;
  }
  const { data } = await apiClient.post<CollabTokenResponse>('/collaboration/token');
  cached = { token: data.token, expiresAt: now + data.expires_in * 1000 };
  return cached.token;
}

/** Drop the cached token (workspace switch / logout). */
export function resetCollabToken(): void {
  cached = null;
}
