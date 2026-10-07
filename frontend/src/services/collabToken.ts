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
/** One fetch shared by every caller that finds the cache empty. */
let inflight: Promise<string> | null = null;
/** Bumped by `resetCollabToken`, so a fetch that finishes after a reset
 *  (a workspace switch, sign-out) doesn't cache the old workspace's token. */
let generation = 0;

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
  const valid = peekCollabToken();
  if (valid) return valid;
  if (!inflight) {
    const startedIn = generation;
    const fetching: Promise<string> = (async () => {
      const now = Date.now();
      const { data } = await apiClient.post<CollabTokenResponse>('/collaboration/token');
      const bufferSecs = data.refresh_buffer ?? data.expires_in * FALLBACK_BUFFER_RATIO;
      // Store the moment we should stop using it, not the raw expiry, so the
      // buffer is applied once here rather than at every read.
      if (startedIn === generation) {
        cached = {
          token: data.token,
          expiresAt: now + Math.max(0, data.expires_in - bufferSecs) * 1000,
        };
      }
      return data.token;
    })().finally(() => {
      if (inflight === fetching) inflight = null;
    });
    inflight = fetching;
  }
  return inflight;
}

/** The cached token while it is still good to connect with, else null. */
export function peekCollabToken(): string | null {
  return cached && Date.now() < cached.expiresAt ? cached.token : null;
}

/** Drop the cached token (workspace switch / logout) and stop keeping one
 *  ready; the next workspace's sync start resumes it. */
export function resetCollabToken(): void {
  generation++;
  cached = null;
  inflight = null;
  stopWarm();
}

// ---- Kept ready -------------------------------------------------------------
//
// With a token at hand, opening a note connects without a token round trip
// first, which matters most where nothing hovers to warm it (mobile, kanban,
// links from elsewhere). One POST each time a token runs out (about every 90s),
// only while the page is visible.

let warming = false;
let warmTimer: ReturnType<typeof setTimeout> | null = null;

function stopWarm(): void {
  warming = false;
  if (warmTimer) clearTimeout(warmTimer);
  warmTimer = null;
}

function pageHidden(): boolean {
  return typeof document !== 'undefined' && document.visibilityState === 'hidden';
}

async function refreshWarmToken(): Promise<void> {
  if (warmTimer) clearTimeout(warmTimer);
  warmTimer = null;
  if (!warming || pageHidden()) return;
  const startedIn = generation;
  try {
    await getCollabToken();
  } catch {
    // Signed out, or the API is down: stop rather than retry in the
    // background. Opening a note still fetches one.
    if (startedIn === generation) stopWarm();
    return;
  }
  if (!warming || startedIn !== generation || !cached) return;
  // Just after it stops being good to connect with, when a fetch replaces it.
  warmTimer = setTimeout(() => void refreshWarmToken(), Math.max(1000, cached.expiresAt - Date.now() + 50));
}

/**
 * Keep a collab token ready from now on: fetch one at once and another as
 * each runs out, while the page is visible. Called once the sync runtime has
 * loaded a workspace; calling it again does nothing extra.
 */
export function keepCollabTokenWarm(): void {
  if (warming) return;
  warming = true;
  void refreshWarmToken();
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (warming && !pageHidden()) void refreshWarmToken();
  });
}
