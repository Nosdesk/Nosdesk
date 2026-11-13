/**
 * File tokens, and the URL a video streams from with one.
 *
 * In the app, file loads go through the `nosdesk-asset` proxy (see
 * `mobile/src-tauri/src/asset_proxy.rs`), which can't stream: it fetches each
 * range whole before the player sees a byte. A video instead loads straight
 * from the server, so the system player streams, buffers and seeks on its own.
 * The webview can't send the bearer on a media load, so the URL carries a file
 * token: read-only, for the one file path it was minted for, bound to the
 * workspace it was minted in, so it is reset on a workspace switch and on
 * sign-out (`resetFileToken`).
 */
import apiClient from '@nosdesk/core/apiClient';
import { apiBaseUrl, assetPath } from '@nosdesk/core/transport';
import { serverMintsFileTokens } from '@nosdesk/core/services/instanceConfig';
import { isTauriRuntime } from '@/platform';

interface FileTokenResponse {
  token: string;
  expires_in: number;
  /** Seconds before expiry at which to re-mint. Served by the backend. */
  refresh_buffer?: number;
}

/** Tokens by the file path they open. */
const cached = new Map<string, { token: string; expiresAt: number }>();
/** One fetch per path, shared by every caller that finds that path's cache
 *  empty. */
const inflight = new Map<string, Promise<string>>();
/** Bumped by `resetFileToken`, so a fetch that finishes after a reset doesn't
 *  cache the old workspace's token. */
let generation = 0;
const TOKEN_FETCH_TIMEOUT_MS = 10_000;
const FALLBACK_BUFFER_RATIO = 0.5;

/**
 * A file token for `path` (the `/api/files/...` path the element loads),
 * cached until its refresh point. The backend serves that point with the
 * token: early enough that a video starts with most of a token's life ahead
 * of it.
 */
export async function getFileToken(path: string): Promise<string> {
  const hit = cached.get(path);
  if (hit && Date.now() < hit.expiresAt) return hit.token;
  let fetching = inflight.get(path);
  if (!fetching) {
    const startedIn = generation;
    const request: Promise<string> = (async () => {
      const now = Date.now();
      const { data } = await apiClient.post<FileTokenResponse>(
        '/files/token',
        { path },
        { timeout: TOKEN_FETCH_TIMEOUT_MS },
      );
      const bufferSecs = data.refresh_buffer ?? data.expires_in * FALLBACK_BUFFER_RATIO;
      if (startedIn === generation) {
        cached.set(path, {
          token: data.token,
          expiresAt: now + Math.max(0, data.expires_in - bufferSecs) * 1000,
        });
      }
      return data.token;
    })().finally(() => {
      if (inflight.get(path) === request) inflight.delete(path);
    });
    inflight.set(path, request);
    fetching = request;
  }
  return fetching;
}

/** Drop every cached token (workspace switch, sign-out). */
export function resetFileToken(): void {
  generation++;
  cached.clear();
  inflight.clear();
}

/** Drop `path`'s cached token after a load refused it, so the next one
 *  re-mints. */
export function discardFileToken(path: string): void {
  cached.delete(path);
}

/** A video's direct load: the server `url` to stream from, without its token,
 *  and the `path` to mint that token for. */
export interface DirectVideo {
  url: string;
  path: string;
}

/**
 * Where a video at `src` can stream from directly, or null to keep `src`.
 * Only in the app, for a server file that the app would otherwise proxy, on a
 * server that mints file tokens, over https (the app's media policy allows no
 * other remote origin).
 */
export function directVideoUrl(src: string): DirectVideo | null {
  if (!isTauriRuntime() || !serverMintsFileTokens()) return null;
  // `src` has been through `assetUrl`; a proxied one maps back to its path.
  const path = assetPath(src);
  if (path === src || !path.startsWith('/api/files/')) return null;
  let base: URL;
  try {
    base = new URL(apiBaseUrl());
  } catch {
    return null;
  }
  if (base.protocol !== 'https:') return null;
  // The token opens exactly the path the element loads.
  return { url: `${base.origin}${path}`, path };
}

/** `url` carrying `token` as its `token` query parameter. */
export function withFileToken(url: string, token: string): string {
  return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`;
}
